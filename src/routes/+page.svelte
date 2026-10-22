<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open as pickFolder } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";

  type NowPlaying = {
    title: string;
    artist: string;
    album: string;
    playing: boolean;
    position: number;
    duration: number;
    cover: string | null;
    canNext: boolean;
    canPrevious: boolean;
  };
  type TrackInfo = {
    path: string;
    title: string;
    artist: string;
    album: string;
    number: number | null;
    duration: number;
  };
  type FolderEntry = { path: string; name: string };
  type FolderView = { path: string; name: string; folders: FolderEntry[]; tracks: TrackInfo[] };
  type QueueState = { items: { path: string; title: string }[]; index: number };

  // ---- settings (per-viewer, stored locally) ----
  const SETTINGS_KEY = "carina.settings";
  function loadSettings() {
    try {
      const s = JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? "");
      if (typeof s?.opacity === "number" && typeof s?.volume === "number") return s;
    } catch {}
    return { opacity: 40, volume: 0.8 };
  }
  let settings = $state<{ opacity: number; volume: number }>(loadSettings());
  let settingsOpen = $state(false);
  $effect(() => {
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    } catch {}
  });

  // ---- now playing ----
  let track = $state<NowPlaying | null>(null);
  let position = $state(0);
  let receivedAt = 0;
  const progress = $derived(
    track && track.duration > 0 ? Math.min(1, position / track.duration) : 0,
  );

  // ---- library panel ----
  let panelOpen = $state(false);
  let up = $state(false);
  let tab = $state<"library" | "queue">("library");
  let roots = $state<FolderEntry[]>([]);
  let view = $state<FolderView | null>(null);
  let history: string[] = [];
  let covers = $state<Record<string, string | null>>({});
  let query = $state("");
  let results = $state<TrackInfo[] | null>(null);
  let queue = $state<QueueState>({ items: [], index: 0 });
  let notice = $state("");
  let volumeShown = $state(false);
  let volumeTimer: ReturnType<typeof setTimeout>;

  const own = $derived(queue.items.length > 0);
  const playingPath = $derived(own ? queue.items[queue.index]?.path : undefined);

  function clock(seconds: number) {
    const s = Math.max(0, Math.floor(seconds));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }

  function say(text: string) {
    notice = text;
    setTimeout(() => notice === text && (notice = ""), 2500);
  }

  async function togglePanel() {
    settingsOpen = false;
    const dir = await invoke<string>("set_panel", { open: !panelOpen });
    panelOpen = !panelOpen;
    up = dir === "up";
    if (panelOpen && !view) await showRoots();
  }

  async function showRoots() {
    view = null;
    history = [];
    roots = await invoke<FolderEntry[]>("library_roots");
  }

  async function openFolder(path: string, remember = true) {
    try {
      const next = await invoke<FolderView>("library_open", { path });
      if (remember && view) history.push(view.path);
      if (remember && !view) history = [];
      view = next;
      query = "";
      results = null;
    } catch (e) {
      say(String(e));
    }
  }

  async function back() {
    const prev = history.pop();
    if (prev) await openFolder(prev, false);
    else await showRoots();
  }

  async function addFolder(path?: string) {
    const picked = path ?? (await pickFolder({ directory: true, title: "Папка с музыкой" }));
    if (typeof picked !== "string") return false;
    try {
      await invoke("library_add", { path: picked });
      await showRoots();
      return true;
    } catch (e) {
      say(String(e));
      return false;
    }
  }

  async function removeRoot(path: string) {
    await invoke("library_remove", { path });
    await showRoots();
  }

  function playFolder(path: string, start?: string) {
    invoke("play_folder", { path, start }).catch((e) => say(String(e)));
  }

  function playTrack(t: TrackInfo, i: number) {
    if (results) invoke("play_tracks", { paths: results.map((r) => r.path), start: i });
    else if (view) playFolder(view.path, t.path);
  }

  // Covers load lazily, one folder at a time.
  $effect(() => {
    const paths = (view ? view.folders : roots).map((f) => f.path);
    (async () => {
      for (const p of paths) {
        if (p in covers) continue;
        covers[p] = null;
        covers[p] = await invoke<string | null>("library_cover", { path: p }).catch(() => null);
      }
    })();
  });

  // Search the open folder (or every library folder) as the user types.
  let searchTimer: ReturnType<typeof setTimeout>;
  $effect(() => {
    const q = query.trim();
    clearTimeout(searchTimer);
    if (!q) {
      results = null;
      return;
    }
    const scope = view ? [view.path] : roots.map((r) => r.path);
    searchTimer = setTimeout(async () => {
      const found: TrackInfo[] = [];
      for (const path of scope) {
        found.push(...(await invoke<TrackInfo[]>("library_search", { path, query: q })));
      }
      if (query.trim() === q) results = found;
    }, 250);
  });

  function onWheel(e: WheelEvent) {
    if (!own) return;
    e.preventDefault();
    const v = settings.volume - Math.sign(e.deltaY) * 0.05;
    settings.volume = Math.round(Math.min(1, Math.max(0, v)) * 100) / 100;
    invoke("set_volume", { volume: settings.volume });
    volumeShown = true;
    clearTimeout(volumeTimer);
    volumeTimer = setTimeout(() => (volumeShown = false), 1200);
  }

  function seek(e: MouseEvent) {
    if (!track || track.duration <= 0) return;
    const bar = e.currentTarget as HTMLElement;
    const fraction = e.offsetX / bar.clientWidth;
    invoke("media_seek", { seconds: fraction * track.duration });
  }

  onMount(() => {
    invoke("set_volume", { volume: settings.volume });
    const unlisten = [
      listen<NowPlaying>("now-playing", (e) => {
        track = e.payload.title || e.payload.artist ? e.payload : null;
        position = e.payload.position;
        receivedAt = performance.now();
      }),
      listen<QueueState>("queue", (e) => (queue = e.payload)),
      // Drop a music folder on the widget: add it and play it.
      getCurrentWebview().onDragDropEvent(async (e) => {
        if (e.payload.type !== "drop") return;
        for (const path of e.payload.paths) {
          if (await addFolder(path)) {
            playFolder(path);
            break;
          }
        }
      }),
    ];

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && panelOpen) togglePanel();
    };
    window.addEventListener("keydown", onKey);

    let frame = requestAnimationFrame(function tick() {
      if (track?.playing) {
        position = Math.min(
          track.duration || Infinity,
          track.position + (performance.now() - receivedAt) / 1000,
        );
      }
      frame = requestAnimationFrame(tick);
    });

    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("keydown", onKey);
      unlisten.forEach((u) => u.then((f) => f()));
    };
  });
</script>

{#snippet trackRow(t: TrackInfo, i: number)}
  <button class="row" class:current={t.path === playingPath} onclick={() => playTrack(t, i)}>
    <span class="num">{t.path === playingPath && track?.playing ? "♪" : (t.number ?? i + 1)}</span>
    <span class="names">
      <span class="t">{t.title}</span>
      {#if t.artist}<span class="a">{t.artist}</span>{/if}
    </span>
    <span class="dur">{t.duration ? clock(t.duration) : ""}</span>
  </button>
{/snippet}

{#snippet tile(f: FolderEntry, removable: boolean)}
  <div class="tile">
    <button class="art" onclick={() => openFolder(f.path)} aria-label={f.name}>
      {#if covers[f.path]}
        <img src={covers[f.path]} alt="" draggable="false" />
      {:else}
        <svg viewBox="0 0 24 24" class="folder-icon"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
      {/if}
    </button>
    <button class="tile-play" aria-label="Играть папку" onclick={() => playFolder(f.path)}>
      <svg viewBox="0 0 24 24"><path d="M8.5 5.6v12.8a1 1 0 0 0 1.5.86l10.2-6.4a1 1 0 0 0 0-1.72L10 4.74A1 1 0 0 0 8.5 5.6z" /></svg>
    </button>
    {#if removable}
      <button class="tile-remove" aria-label="Убрать из библиотеки" title="Убрать из библиотеки" onclick={() => removeRoot(f.path)}>×</button>
    {/if}
    <span class="tile-name">{f.name}</span>
  </div>
{/snippet}

<main class="widget" class:open={panelOpen} class:up style:--glass={settings.opacity / 100}>
  <div class="card" data-tauri-drag-region onwheel={onWheel}>
    <button class="corner gear" class:active={settingsOpen} aria-label="Настройки" onclick={() => (settingsOpen = !settingsOpen)}>
      <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3" /><path d="M12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1 7 17M17 7l2.1-2.1" /></svg>
    </button>
    <button class="corner library" class:active={panelOpen} aria-label="Библиотека" title="Библиотека" onclick={togglePanel}>
      <svg viewBox="0 0 24 24"><path d="M4 6h16M4 12h10M4 18h7" /><circle cx="18" cy="17" r="2.5" /><path d="M20.5 17V10" /></svg>
    </button>

    <div class="cover" data-tauri-drag-region>
      {#if track?.cover}
        <img src={track.cover} alt="" draggable="false" />
      {:else}
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 18V5l12-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="16" r="3" /></svg>
      {/if}
    </div>

    {#if settingsOpen}
      <section class="settings">
        <label class="setting">
          <span>Фон</span>
          <input type="range" min="0" max="100" step="1" bind:value={settings.opacity} />
          <span class="value">{settings.opacity}%</span>
        </label>
        <button class="done" onclick={() => (settingsOpen = false)}>Готово</button>
      </section>
    {:else}
      <section class="info" data-tauri-drag-region>
        <div class="text" data-tauri-drag-region>
          <div class="title" data-tauri-drag-region>{track?.title || "Тишина"}</div>
          <div class="artist" data-tauri-drag-region>
            {track ? track.artist || track.album || "Неизвестный исполнитель" : "Открой библиотеку и выбери музыку"}
          </div>
        </div>

        <div class="bottom" data-tauri-drag-region>
          <nav class="controls">
            <button class="skip" aria-label="Назад" disabled={!track?.canPrevious} onclick={() => invoke("media_previous")}>
              <svg viewBox="0 0 24 24"><rect x="5" y="5.5" width="2.6" height="13" rx="1.2" /><path d="M19 6.6v10.8a1 1 0 0 1-1.55.83L9.6 13a1.2 1.2 0 0 1 0-2l7.85-5.23A1 1 0 0 1 19 6.6z" /></svg>
            </button>
            <button class="play" aria-label={track?.playing ? "Пауза" : "Играть"} onclick={() => invoke("media_toggle")}>
              {#if track?.playing}
                <svg viewBox="0 0 24 24"><rect x="6.5" y="5" width="4" height="14" rx="1.4" /><rect x="13.5" y="5" width="4" height="14" rx="1.4" /></svg>
              {:else}
                <svg viewBox="0 0 24 24"><path d="M8.5 5.6v12.8a1 1 0 0 0 1.5.86l10.2-6.4a1 1 0 0 0 0-1.72L10 4.74A1 1 0 0 0 8.5 5.6z" /></svg>
              {/if}
            </button>
            <button
              class="skip"
              aria-label="Вперёд"
              disabled={!track?.canNext}
              title={track && !track.canNext ? "Дальше в очереди ничего нет" : undefined}
              onclick={() => invoke("media_next")}
            >
              <svg viewBox="0 0 24 24"><rect x="16.4" y="5.5" width="2.6" height="13" rx="1.2" /><path d="M5 6.6v10.8a1 1 0 0 0 1.55.83L14.4 13a1.2 1.2 0 0 0 0-2L6.55 5.77A1 1 0 0 0 5 6.6z" /></svg>
            </button>
          </nav>
          <span class="time" class:hidden={!volumeShown && (!track || track.duration <= 0)} data-tauri-drag-region>
            {#if volumeShown}Громкость {Math.round(settings.volume * 100)}%{:else}{clock(position)} <i>/</i> {clock(track?.duration ?? 0)}{/if}
          </span>
        </div>
      </section>
    {/if}

    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="progress" class:hidden={!track || track.duration <= 0} onclick={seek}>
      <div class="line"><div class="fill" style:width="{progress * 100}%"></div></div>
    </div>
  </div>

  {#if panelOpen}
    <section class="panel">
      <header data-tauri-drag-region>
        {#if view && tab === "library"}
          <button class="icon" aria-label="Назад" onclick={back}>
            <svg viewBox="0 0 24 24"><path d="M15 5l-7 7 7 7" /></svg>
          </button>
        {/if}
        <div class="heading" data-tauri-drag-region>{tab === "queue" ? "Очередь" : (view?.name ?? "Библиотека")}</div>
        <div class="tabs">
          <button class:on={tab === "library"} onclick={() => (tab = "library")}>Папки</button>
          <button class:on={tab === "queue"} onclick={() => (tab = "queue")}>Очередь{own ? ` · ${queue.items.length}` : ""}</button>
        </div>
      </header>

      {#if tab === "library"}
        <div class="search">
          <svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="6.5" /><path d="M20 20l-4.2-4.2" /></svg>
          <input placeholder={view ? `Искать в «${view.name}»` : "Искать во всей музыке"} bind:value={query} spellcheck="false" />
        </div>
      {/if}

      <div class="scroll">
        {#if tab === "queue"}
          {#if own}
            {#each queue.items as item, i (i)}
              <button class="row" class:current={i === queue.index} onclick={() => invoke("queue_jump", { index: i })}>
                <span class="num">{i === queue.index && track?.playing ? "♪" : i + 1}</span>
                <span class="names"><span class="t">{item.title}</span></span>
              </button>
            {/each}
          {:else}
            <p class="empty">Очередь пуста. Выбери папку или трек в «Папках».</p>
          {/if}
        {:else if results}
          {#each results as t, i (t.path)}{@render trackRow(t, i)}{/each}
          {#if results.length === 0}<p class="empty">Ничего не нашлось</p>{/if}
        {:else if !view}
          <div class="grid">
            {#each roots as f (f.path)}{@render tile(f, true)}{/each}
            <button class="tile add" onclick={() => addFolder()}>
              <span class="art"><svg viewBox="0 0 24 24"><path d="M12 6v12M6 12h12" /></svg></span>
              <span class="tile-name">Добавить папку</span>
            </button>
          </div>
          {#if roots.length === 0}
            <p class="empty">Добавь папку с музыкой — каждая папка станет плейлистом. Можно просто перетащить её на виджет.</p>
          {/if}
        {:else}
          {#if view.folders.length}
            <div class="grid">
              {#each view.folders as f (f.path)}{@render tile(f, false)}{/each}
            </div>
          {/if}
          {#if view.tracks.length}
            <button class="play-all" onclick={() => playFolder(view!.path)}>
              <svg viewBox="0 0 24 24"><path d="M8.5 5.6v12.8a1 1 0 0 0 1.5.86l10.2-6.4a1 1 0 0 0 0-1.72L10 4.74A1 1 0 0 0 8.5 5.6z" /></svg>
              Играть всё
            </button>
            {#each view.tracks as t, i (t.path)}{@render trackRow(t, i)}{/each}
          {/if}
        {/if}
      </div>

      {#if notice}<div class="notice">{notice}</div>{/if}
    </section>
  {/if}
</main>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
    background: transparent;
    user-select: none;
    -webkit-user-select: none;
    cursor: default;
    font-family: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
    color: #fff;
  }

  /* ---- the glass: transparent window, tint painted here ---- */
  .widget {
    --glass: 0.4;
    box-sizing: border-box;
    height: 100vh;
    display: flex;
    flex-direction: column;
    border-radius: 10px;
    overflow: hidden;
    background-color: rgb(14 16 24 / calc(0.9 * var(--glass)));
    background-image: linear-gradient(135deg, rgb(255 255 255 / 0.05), rgb(255 255 255 / 0));
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.12);
  }
  .widget.up {
    flex-direction: column-reverse;
  }

  .card {
    position: relative;
    flex: none;
    box-sizing: border-box;
    height: 132px;
    display: flex;
    gap: 16px;
    padding: 16px;
  }

  .corner {
    position: absolute;
    right: 6px;
    width: 22px;
    height: 22px;
    opacity: 0;
    transition: opacity 0.2s;
  }
  .corner.gear {
    top: 6px;
  }
  .corner.library {
    bottom: 10px;
  }
  .card:hover .corner,
  .corner.active {
    opacity: 1;
  }
  .corner svg {
    width: 13px;
    height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
  }

  .cover {
    flex: none;
    width: 100px;
    height: 100px;
    border-radius: 6px;
    overflow: hidden;
    display: grid;
    place-items: center;
    background: linear-gradient(135deg, rgb(255 255 255 / 0.12), rgb(255 255 255 / 0.04));
    box-shadow: 0 6px 18px rgb(0 0 0 / 0.35);
  }
  .cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .cover svg {
    width: 36px;
    height: 36px;
    fill: none;
    stroke: rgb(255 255 255 / 0.55);
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
  }
  .text {
    padding-right: 18px;
  }
  .title,
  .artist {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-shadow: 0 1px 6px rgb(0 0 0 / 0.5);
  }
  .title {
    font-size: 15px;
    font-weight: 600;
    letter-spacing: 0.01em;
  }
  .artist {
    margin-top: 2px;
    font-size: 13px;
    color: rgb(255 255 255 / 0.75);
  }

  .bottom {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-right: 22px;
  }
  .time {
    margin-left: auto;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: rgb(255 255 255 / 0.6);
    white-space: nowrap;
    text-shadow: 0 1px 4px rgb(0 0 0 / 0.5);
  }
  .time i {
    font-style: normal;
    opacity: 0.5;
  }
  .hidden {
    visibility: hidden;
  }

  /* Progress: a hairline along the bottom edge, with a taller invisible hit area for seeking. */
  .progress {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 8px;
    display: flex;
    align-items: flex-end;
    cursor: pointer;
  }
  .progress .line {
    width: 100%;
    height: 2px;
    background: rgb(255 255 255 / 0.1);
    transition: height 0.15s;
  }
  .progress:hover .line {
    height: 4px;
  }
  .progress .fill {
    height: 100%;
    background: linear-gradient(90deg, rgb(255 255 255 / 0.55), #fff);
    box-shadow: 0 0 8px rgb(255 255 255 / 0.6);
    pointer-events: none;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-left: -6px;
  }
  button {
    width: 30px;
    height: 30px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: transparent;
    color: rgb(255 255 255 / 0.8);
    font: inherit;
    transition:
      background 0.15s,
      color 0.15s,
      transform 0.12s,
      box-shadow 0.2s,
      opacity 0.2s;
  }
  button:hover {
    background: rgb(255 255 255 / 0.1);
    color: #fff;
  }
  button:active {
    transform: scale(0.9);
  }
  button:focus {
    outline: none;
  }
  button:focus-visible {
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.35);
  }
  button:disabled {
    opacity: 0.3;
    background: transparent;
    transform: none;
  }
  button svg {
    width: 15px;
    height: 15px;
    fill: currentColor;
  }
  button.play {
    width: 38px;
    height: 38px;
    background: rgb(255 255 255 / 0.92);
    color: #10121a;
    box-shadow: 0 4px 14px rgb(0 0 0 / 0.35);
  }
  button.play:hover {
    background: #fff;
    color: #10121a;
    transform: scale(1.06);
    box-shadow:
      0 4px 14px rgb(0 0 0 / 0.35),
      0 0 18px rgb(255 255 255 / 0.35);
  }
  button.play:active {
    transform: scale(0.94);
  }
  button.play svg {
    width: 17px;
    height: 17px;
  }

  /* ---- settings ---- */
  .settings {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 14px;
    padding-right: 18px;
    font-size: 12px;
    color: rgb(255 255 255 / 0.85);
  }
  .setting {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .setting .value {
    width: 34px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  input[type="range"] {
    flex: 1;
    min-width: 0;
    accent-color: #fff;
  }
  .done {
    align-self: flex-end;
    width: auto;
    height: 24px;
    padding: 0 12px;
    border-radius: 12px;
    background: rgb(255 255 255 / 0.14);
  }

  /* ---- library panel ---- */
  .panel {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-top: 1px solid rgb(255 255 255 / 0.1);
  }
  .widget.up .panel {
    border-top: 0;
    border-bottom: 1px solid rgb(255 255 255 / 0.1);
  }
  header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 12px 12px 8px 16px;
  }
  .heading {
    flex: 1;
    min-width: 0;
    font-size: 14px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-shadow: 0 1px 4px rgb(0 0 0 / 0.5);
  }
  button.icon {
    width: 26px;
    height: 26px;
    margin-left: -6px;
  }
  button.icon svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .tabs {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 14px;
    background: rgb(255 255 255 / 0.08);
  }
  .tabs button {
    width: auto;
    height: 24px;
    padding: 0 10px;
    border-radius: 12px;
    font-size: 12px;
    color: rgb(255 255 255 / 0.7);
  }
  .tabs button.on {
    background: rgb(255 255 255 / 0.9);
    color: #10121a;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 16px 8px;
    padding: 0 10px;
    height: 30px;
    border-radius: 8px;
    background: rgb(255 255 255 / 0.08);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.08);
  }
  .search:focus-within {
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.3);
  }
  .search svg {
    flex: none;
    width: 14px;
    height: 14px;
    fill: none;
    stroke: rgb(255 255 255 / 0.55);
    stroke-width: 2;
    stroke-linecap: round;
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    color: #fff;
    font: inherit;
    font-size: 13px;
    user-select: text;
  }
  .search input::placeholder {
    color: rgb(255 255 255 / 0.45);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 10px 12px 16px;
    scrollbar-width: thin;
    scrollbar-color: rgb(255 255 255 / 0.25) transparent;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px 10px;
    margin-bottom: 12px;
  }
  .tile {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .tile .art {
    width: 100%;
    height: auto;
    aspect-ratio: 1;
    border-radius: 8px;
    overflow: hidden;
    display: grid;
    place-items: center;
    background: linear-gradient(135deg, rgb(255 255 255 / 0.14), rgb(255 255 255 / 0.04));
    box-shadow: 0 4px 12px rgb(0 0 0 / 0.3);
  }
  .tile .art:active {
    transform: scale(0.97);
  }
  .tile .art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .tile .art svg {
    width: 34px;
    height: 34px;
    fill: none;
    stroke: rgb(255 255 255 / 0.55);
    stroke-width: 1.5;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  .tile-name {
    font-size: 12px;
    line-height: 1.25;
    color: rgb(255 255 255 / 0.85);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    text-shadow: 0 1px 4px rgb(0 0 0 / 0.5);
  }
  .tile.add {
    width: auto;
    height: auto;
    border-radius: 0;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
    text-align: left;
  }
  .tile.add:hover {
    background: transparent;
  }
  .tile.add .art {
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.2);
    background: rgb(255 255 255 / 0.04);
  }
  .tile-play {
    position: absolute;
    right: 6px;
    top: 6px;
    width: 30px;
    height: 30px;
    background: rgb(255 255 255 / 0.92);
    color: #10121a;
    opacity: 0;
    box-shadow: 0 3px 10px rgb(0 0 0 / 0.4);
  }
  .tile:hover .tile-play {
    opacity: 1;
  }
  .tile-play:hover {
    background: #fff;
    color: #10121a;
  }
  .tile-play svg {
    width: 13px;
    height: 13px;
  }
  .tile-remove {
    position: absolute;
    top: 6px;
    left: 6px;
    width: 20px;
    height: 20px;
    font-size: 14px;
    line-height: 1;
    background: rgb(0 0 0 / 0.5);
    opacity: 0;
  }
  .tile:hover .tile-remove {
    opacity: 1;
  }

  .play-all {
    width: auto;
    height: 28px;
    margin: 0 0 6px;
    padding: 0 12px 0 8px;
    border-radius: 14px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    background: rgb(255 255 255 / 0.1);
  }
  .play-all svg {
    width: 12px;
    height: 12px;
  }

  .row {
    width: 100%;
    height: 40px;
    padding: 0 8px 0 0;
    border-radius: 8px;
    display: flex;
    align-items: center;
    gap: 8px;
    text-align: left;
  }
  .row:active {
    transform: none;
  }
  .row.current {
    background: rgb(255 255 255 / 0.12);
  }
  .row .num {
    flex: none;
    width: 26px;
    text-align: center;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: rgb(255 255 255 / 0.5);
  }
  .row.current .num {
    color: #fff;
  }
  .row .names {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .row .t,
  .row .a {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-shadow: 0 1px 4px rgb(0 0 0 / 0.5);
  }
  .row .t {
    font-size: 13px;
  }
  .row .a {
    font-size: 11px;
    color: rgb(255 255 255 / 0.55);
  }
  .row .dur {
    flex: none;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: rgb(255 255 255 / 0.5);
  }

  .empty {
    margin: 12px 4px;
    font-size: 12px;
    line-height: 1.5;
    color: rgb(255 255 255 / 0.65);
    text-shadow: 0 1px 4px rgb(0 0 0 / 0.5);
  }
  .notice {
    position: absolute;
    left: 16px;
    right: 16px;
    bottom: 12px;
    padding: 8px 12px;
    border-radius: 8px;
    font-size: 12px;
    background: rgb(20 22 30 / 0.92);
    box-shadow: 0 6px 18px rgb(0 0 0 / 0.4);
  }
</style>
