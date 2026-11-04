<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open as pickFolder } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { i18n, t, type Key } from "$lib/i18n.svelte";
  import { parseSettings, type Lang, type Settings } from "$lib/settings";

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
  type Repeat = "off" | "all" | "one";
  type QueueState = {
    items: { path: string; title: string }[];
    index: number;
    shuffle: boolean;
    repeat: Repeat;
  };
  type PlaylistSummary = { id: string; name: string; count: number };

  // ---- settings (per-viewer, stored locally) ----
  const SETTINGS_KEY = "carina.settings";
  function loadSettings(): Settings {
    try {
      return parseSettings(localStorage.getItem(SETTINGS_KEY));
    } catch {
      return parseSettings(null);
    }
  }
  let settings = $state<Settings>(loadSettings());
  i18n.lang = settings.lang;
  let settingsOpen = $state(false);
  $effect(() => {
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    } catch {}
  });

  // The tray menu follows the widget's language; the backend keeps it for the next start.
  const syncTray = (lang: Lang) => invoke("set_tray_language", { lang }).catch(() => {});
  syncTray(settings.lang);
  $effect(() => {
    document.documentElement.lang = i18n.lang;
  });

  function setLang(lang: Lang) {
    settings.lang = lang;
    i18n.lang = lang;
    syncTray(lang);
  }

  // Names of the built-in Favorites list are translated; user lists keep their own names.
  const listName = (l: PlaylistSummary) => (l.id === "favorites" ? t("favorites") : l.name);

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
  let tab = $state<"library" | "playlists" | "queue">("library");
  let roots = $state<FolderEntry[]>([]);
  let view = $state<FolderView | null>(null);
  let history: string[] = [];
  let covers = $state<Record<string, string | null>>({});
  let query = $state("");
  let results = $state<TrackInfo[] | null>(null);
  let queue = $state<QueueState>({ items: [], index: 0, shuffle: false, repeat: "off" });
  let lists = $state<PlaylistSummary[]>([]);
  let openList = $state<PlaylistSummary | null>(null);
  let listTracks = $state<TrackInfo[]>([]);
  let favorites = $state<Set<string>>(new Set());
  let naming = $state<{ id: string | null; name: string; paths: string[] } | null>(null);
  let menu = $state<{
    paths: string[];
    folder: FolderEntry | null;
    x: number;
    y: number;
    lists: boolean;
  } | null>(null);
  let listCovers = $state<Record<string, string | null>>({});
  let drag = $state<{ from: number; over: number } | null>(null);
  let notice = $state("");
  let volumeShown = $state(false);
  let volumeTimer: ReturnType<typeof setTimeout>;

  const own = $derived(queue.items.length > 0);
  const playingPath = $derived(own ? queue.items[queue.index]?.path : undefined);
  const heading = $derived(
    tab === "queue"
      ? t("tabQueue")
      : tab === "playlists"
        ? (openList ? listName(openList) : t("tabPlaylists"))
        : (view?.name ?? t("library")),
  );

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

  // The backend reports these errors in English; they are translated here by exact text for the Russian interface.
  // Keep the texts in sync with the Rust side (library.rs, engine.rs).
  const backendErrors: Record<string, Key> = {
    "Folder is not in the library": "errNotInLibrary",
    "Not a folder": "errNotFolder",
    "No music in this folder": "errNoMusic",
  };
  function errorText(e: unknown): string {
    const text = String(e);
    const key = backendErrors[text];
    return key ? t(key) : text;
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
      say(errorText(e));
    }
  }

  async function back() {
    const prev = history.pop();
    if (prev) await openFolder(prev, false);
    else await showRoots();
  }

  async function addFolder(path?: string) {
    const picked = path ?? (await pickFolder({ directory: true, title: t("pickFolderTitle") }));
    if (typeof picked !== "string") return false;
    try {
      await invoke("library_add", { path: picked });
      await showRoots();
      return true;
    } catch (e) {
      say(errorText(e));
      return false;
    }
  }

  async function removeRoot(path: string) {
    await invoke("library_remove", { path });
    await showRoots();
  }

  function playFolder(path: string, start?: string) {
    invoke("play_folder", { path, start }).catch((e) => say(errorText(e)));
  }

  function playTrack(t: TrackInfo, i: number) {
    if (tab === "playlists") invoke("play_tracks", { paths: listTracks.map((r) => r.path), start: i });
    else if (results) invoke("play_tracks", { paths: results.map((r) => r.path), start: i });
    else if (view) playFolder(view.path, t.path);
  }

  // ---- playlists & favorites ----
  async function loadLists() {
    lists = await invoke<PlaylistSummary[]>("playlists");
    favorites = new Set(await invoke<string[]>("favorites"));
  }

  async function showList(l: PlaylistSummary) {
    openList = l;
    listTracks = await invoke<TrackInfo[]>("playlist_tracks", { id: l.id });
  }

  async function toggleFavorite(path: string) {
    const on = await invoke<boolean>("favorite_toggle", { path });
    const next = new Set(favorites);
    if (on) next.add(path);
    else next.delete(path);
    favorites = next;
    if (openList?.id === "favorites") await showList(openList);
    lists = await invoke<PlaylistSummary[]>("playlists");
  }

  async function saveName() {
    if (!naming) return;
    const { id, name, paths } = naming;
    naming = null;
    if (id) {
      await invoke("playlist_rename", { id, name });
      if (openList?.id === id) openList = { ...openList, name };
    } else {
      // Named here, in the widget's language, so the saved name stays in that language.
      // `lists` includes Favorites, so its length is the next number the backend would pick.
      const title = name.trim() || t("playlistNumbered", { n: lists.length });
      const created = await invoke<PlaylistSummary>("playlist_create", { name: title, paths });
      if (paths.length) say(t("playlistCreated", { name: created.name }));
    }
    await loadLists();
  }

  async function deleteList(l: PlaylistSummary) {
    await invoke("playlist_delete", { id: l.id });
    openList = null;
    await loadLists();
  }

  /// Tracks the open menu acts on: the clicked track, or a whole folder with its subfolders.
  async function menuPaths(): Promise<string[]> {
    if (!menu) return [];
    if (menu.folder) return invoke<string[]>("folder_tracks", { path: menu.folder.path });
    return menu.paths;
  }

  async function addToList(l: PlaylistSummary) {
    const paths = await menuPaths();
    menu = null;
    const added = await invoke<number>("playlist_add", { id: l.id, paths });
    say(added ? t("addedToList", { count: added, name: listName(l) }) : t("alreadyInList", { name: listName(l) }));
    await loadLists();
  }

  /// A folder becomes a playlist in one click, named after the folder.
  async function newListFromFolder(folder: FolderEntry) {
    const paths = await menuPaths();
    menu = null;
    const created = await invoke<PlaylistSummary>("playlist_create", { name: folder.name, paths });
    say(t("playlistFromFolder", { name: created.name, count: paths.length }));
    await loadLists();
  }

  async function playList(l: PlaylistSummary) {
    const tracks = await invoke<TrackInfo[]>("playlist_tracks", { id: l.id });
    if (tracks.length) invoke("play_tracks", { paths: tracks.map((t) => t.path), start: 0 });
  }

  // Playlist tiles get a cover from their first tracks.
  $effect(() => {
    if (tab !== "playlists") return;
    const ids = lists.map((l) => `${l.id}:${l.count}`);
    (async () => {
      for (const key of ids) {
        if (key in listCovers) continue;
        listCovers[key] = null;
        const id = key.slice(0, key.lastIndexOf(":"));
        listCovers[key] = await invoke<string | null>("playlist_cover", { id }).catch(() => null);
      }
    })();
  });

  async function removeFromList(path: string) {
    if (!openList) return;
    await invoke("playlist_remove", { id: openList.id, path });
    await showList(openList);
    lists = await invoke<PlaylistSummary[]>("playlists");
  }

  function openMenu(e: MouseEvent, paths: string[], folder: FolderEntry | null = null, toLists = false) {
    e.preventDefault();
    e.stopPropagation();
    const panel = (e.currentTarget as HTMLElement).closest(".panel")!.getBoundingClientRect();
    const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
    // Right under the button; above it when there is no room below.
    const height = folder ? 140 : 104;
    const below = box.bottom - panel.top + 2;
    menu = {
      paths,
      folder,
      x: Math.max(8, Math.min(box.right - panel.left - 184, panel.width - 196)),
      y: below + height <= panel.height - 8 ? below : Math.max(8, box.top - panel.top - height - 2),
      lists: toLists,
    };
    loadLists();
  }

  async function queueAction(kind: "next" | "add") {
    const paths = await menuPaths();
    menu = null;
    invoke(kind === "next" ? "queue_play_next" : "queue_add", { paths });
    say(kind === "next" ? t("playNextNotice") : t("queuedNotice", { count: paths.length }));
  }

  // ---- reordering by dragging the grip (queue and playlists) ----
  function startDrag(e: PointerEvent, from: number) {
    e.preventDefault();
    drag = { from, over: from };
    const onMove = (ev: PointerEvent) => {
      const hit = document.elementFromPoint(ev.clientX, ev.clientY) as HTMLElement | null;
      const row = hit?.closest<HTMLElement>("[data-index]");
      if (row && drag) drag = { ...drag, over: Number(row.dataset.index) };
    };
    const onUp = async () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      const d = drag;
      drag = null;
      if (!d || d.from === d.over) return;
      if (tab === "queue") invoke("queue_move", { from: d.from, to: d.over });
      else if (openList) {
        await invoke("playlist_move", { id: openList.id, from: d.from, to: d.over });
        await showList(openList);
      }
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }

  function cycleRepeat() {
    const next: Repeat = queue.repeat === "off" ? "all" : queue.repeat === "all" ? "one" : "off";
    invoke("set_repeat", { mode: next });
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
    loadLists();
    const unlisten = [
      listen<NowPlaying>("now-playing", (e) => {
        track = e.payload.title || e.payload.artist ? e.payload : null;
        position = e.payload.position;
        receivedAt = performance.now();
      }),
      listen<QueueState>("queue", (e) => (queue = e.payload)),
      // Drop on the widget: a music folder joins the library and plays; audio files just play.
      getCurrentWebview().onDragDropEvent(async (e) => {
        if (e.payload.type !== "drop") return;
        const audio = /\.(mp3|flac|wav|m4a|aac|ogg|aiff?)$/i;
        const files = e.payload.paths.filter((p) => audio.test(p));
        if (files.length) {
          invoke("open_files", { paths: files });
          return;
        }
        for (const path of e.payload.paths) {
          if (await addFolder(path)) {
            playFolder(path);
            break;
          }
        }
      }),
    ];

    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (menu) menu = null;
      else if (naming) naming = null;
      else if (panelOpen) togglePanel();
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

{#snippet heart(path: string)}
  <button
    class="mini heart"
    class:on={favorites.has(path)}
    aria-label={t("favorites")}
    title={favorites.has(path) ? t("removeFromFavorites") : t("addToFavorites")}
    onclick={(e) => {
      e.stopPropagation();
      toggleFavorite(path);
    }}
  >
    <svg viewBox="0 0 24 24"><path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z" /></svg>
  </button>
{/snippet}

{#snippet trackRow(tr: TrackInfo, i: number, inList: boolean)}
  <div class="row" class:current={tr.path === playingPath} class:over={drag?.over === i && drag.from !== i} data-index={i}>
    {#if inList}
      <span class="grip" role="button" tabindex="-1" aria-label={t("dragToReorder")} title={t("dragToReorder")} onpointerdown={(e) => startDrag(e, i)}>⋮⋮</span>
    {/if}
    <button class="row-main" onclick={() => playTrack(tr, i)}>
      <span class="num">{tr.path === playingPath && track?.playing ? "♪" : inList ? i + 1 : (tr.number ?? i + 1)}</span>
      <span class="names">
        <span class="t">{tr.title}</span>
        {#if tr.artist}<span class="a">{tr.artist}</span>{/if}
      </span>
      <span class="dur">{tr.duration ? clock(tr.duration) : ""}</span>
    </button>
    {@render heart(tr.path)}
    {#if inList && openList?.id !== "favorites"}
      <button class="mini" aria-label={t("removeFromPlaylist")} title={t("removeFromPlaylist")} onclick={() => removeFromList(tr.path)}>×</button>
    {/if}
    <button class="mini" aria-label={t("more")} title={t("more")} onclick={(e) => openMenu(e, [tr.path])}>⋯</button>
  </div>
{/snippet}

{#snippet tile(f: FolderEntry, removable: boolean)}
  <div class="tile" role="group" oncontextmenu={(e) => openMenu(e, [], f)}>
    <button class="art" onclick={() => openFolder(f.path)} aria-label={f.name} title={f.name}>
      {#if covers[f.path]}
        <img src={covers[f.path]} alt="" draggable="false" />
      {:else}
        <svg viewBox="0 0 24 24" class="folder-icon"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
      {/if}
    </button>
    <button class="tile-play" aria-label={t("playFolder")} onclick={() => playFolder(f.path)}>
      <svg viewBox="0 0 24 24"><path d="M8.5 5.6v12.8a1 1 0 0 0 1.5.86l10.2-6.4a1 1 0 0 0 0-1.72L10 4.74A1 1 0 0 0 8.5 5.6z" /></svg>
    </button>
    <button class="tile-more" aria-label={t("more")} title={t("playlistOrQueue")} onclick={(e) => openMenu(e, [], f)}>⋯</button>
    {#if removable}
      <button class="tile-remove" aria-label={t("removeFromLibrary")} title={t("removeFromLibrary")} onclick={() => removeRoot(f.path)}>×</button>
    {/if}
    <span class="tile-name" title={f.name}>{f.name}</span>
  </div>
{/snippet}

<main class="widget" class:open={panelOpen} class:up style:--glass={settings.opacity / 100}>
  <div class="card" data-tauri-drag-region onwheel={onWheel}>
    <button class="corner gear" class:active={settingsOpen} aria-label={t("settings")} onclick={() => (settingsOpen = !settingsOpen)}>
      <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3" /><path d="M12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1 7 17M17 7l2.1-2.1" /></svg>
    </button>
    <button class="corner library" class:active={panelOpen} aria-label={t("library")} title={t("library")} onclick={togglePanel}>
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
          <span>{t("background")}</span>
          <input type="range" min="0" max="100" step="1" bind:value={settings.opacity} />
          <span class="value">{settings.opacity}%</span>
        </label>
        <div class="settings-foot">
          <span class="signature"><span class="by">by</span><span class="author">Zhichkin</span></span>
          <div class="lang" role="group" aria-label={t("language")}>
            <button class:on={i18n.lang === "en"} title={t("langEnglish")} aria-pressed={i18n.lang === "en"} onclick={() => setLang("en")}>EN</button>
            <button class:on={i18n.lang === "ru"} title={t("langRussian")} aria-pressed={i18n.lang === "ru"} onclick={() => setLang("ru")}>RU</button>
          </div>
          <button class="done" onclick={() => (settingsOpen = false)}>{t("done")}</button>
        </div>
      </section>
    {:else}
      <section class="info" data-tauri-drag-region>
        <div class="text" data-tauri-drag-region>
          <div class="title" data-tauri-drag-region>{track?.title || t("nothingPlaying")}</div>
          <div class="artist" data-tauri-drag-region>
            {track ? track.artist || track.album || t("unknownArtist") : t("openLibraryHint")}
          </div>
        </div>

        <div class="bottom" data-tauri-drag-region>
          <nav class="controls">
            <button class="skip" aria-label={t("previous")} disabled={!track?.canPrevious} onclick={() => invoke("media_previous")}>
              <svg viewBox="0 0 24 24"><rect x="5" y="5.5" width="2.6" height="13" rx="1.2" /><path d="M19 6.6v10.8a1 1 0 0 1-1.55.83L9.6 13a1.2 1.2 0 0 1 0-2l7.85-5.23A1 1 0 0 1 19 6.6z" /></svg>
            </button>
            <button class="play" aria-label={track?.playing ? t("pause") : t("play")} onclick={() => invoke("media_toggle")}>
              {#if track?.playing}
                <svg viewBox="0 0 24 24"><rect x="6.5" y="5" width="4" height="14" rx="1.4" /><rect x="13.5" y="5" width="4" height="14" rx="1.4" /></svg>
              {:else}
                <svg viewBox="0 0 24 24"><path d="M8.5 5.6v12.8a1 1 0 0 0 1.5.86l10.2-6.4a1 1 0 0 0 0-1.72L10 4.74A1 1 0 0 0 8.5 5.6z" /></svg>
              {/if}
            </button>
            <button
              class="skip"
              aria-label={t("next")}
              disabled={!track?.canNext}
              title={track && !track.canNext ? t("nextUnavailable") : undefined}
              onclick={() => invoke("media_next")}
            >
              <svg viewBox="0 0 24 24"><rect x="16.4" y="5.5" width="2.6" height="13" rx="1.2" /><path d="M5 6.6v10.8a1 1 0 0 0 1.55.83L14.4 13a1.2 1.2 0 0 0 0-2L6.55 5.77A1 1 0 0 0 5 6.6z" /></svg>
            </button>
          </nav>
          <span class="time" class:hidden={!volumeShown && (!track || track.duration <= 0)} data-tauri-drag-region>
            {#if volumeShown}{t("volume")} {Math.round(settings.volume * 100)}%{:else}{clock(position)} <i>/</i> {clock(track?.duration ?? 0)}{/if}
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
        <div class="head-row" data-tauri-drag-region>
          {#if (view && tab === "library") || (openList && tab === "playlists")}
            <button class="icon" aria-label={t("back")} onclick={() => (tab === "playlists" ? (openList = null) : back())}>
              <svg viewBox="0 0 24 24"><path d="M15 5l-7 7 7 7" /></svg>
            </button>
          {/if}
          <div class="heading" title={heading} data-tauri-drag-region>{heading}</div>
        </div>
        <div class="tabs">
          <button class:on={tab === "library"} onclick={() => (tab = "library")}>{t("tabFolders")}</button>
          <button
            class:on={tab === "playlists"}
            onclick={() => {
              tab = "playlists";
              loadLists();
            }}>{t("tabPlaylists")}</button
          >
          <button class:on={tab === "queue"} onclick={() => (tab = "queue")}>{t("tabQueue")}</button>
        </div>
      </header>

      {#if tab === "library"}
        <div class="search">
          <svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="6.5" /><path d="M20 20l-4.2-4.2" /></svg>
          <input placeholder={view ? t("searchIn", { name: view.name }) : t("searchAll")} bind:value={query} spellcheck="false" />
        </div>
      {/if}

      <div class="scroll">
        {#if tab === "queue"}
          {#if own}
            <div class="toolbar">
              <button class="pill" class:on={queue.shuffle} onclick={() => invoke("set_shuffle", { on: !queue.shuffle })}>
                <svg viewBox="0 0 24 24"><path d="M4 7h3c4 0 6 10 10 10h3M4 17h3c1.6 0 2.8-1.6 3.9-3.6M14 9.6C15 8 16 7 17 7h3M17 4l3 3-3 3M17 14l3 3-3 3" /></svg>
                {t("shuffle")}
              </button>
              <button class="pill" class:on={queue.repeat !== "off"} onclick={cycleRepeat}>
                <svg viewBox="0 0 24 24"><path d="M5 10V8a2 2 0 0 1 2-2h12l-3-3M19 14v2a2 2 0 0 1-2 2H5l3 3" /></svg>
                {queue.repeat === "one" ? t("repeatOne") : queue.repeat === "all" ? t("repeatAll") : t("repeat")}
              </button>
              <span class="count">{t("trackCount", { count: queue.items.length })}</span>
            </div>
            {#each queue.items as item, i (i)}
              <div class="row" class:current={i === queue.index} class:over={drag?.over === i && drag.from !== i} data-index={i}>
                <span class="grip" role="button" tabindex="-1" aria-label={t("dragToReorder")} title={t("dragToReorder")} onpointerdown={(e) => startDrag(e, i)}>⋮⋮</span>
                <button class="row-main" onclick={() => invoke("queue_jump", { index: i })}>
                  <span class="num">{i === queue.index && track?.playing ? "♪" : i + 1}</span>
                  <span class="names"><span class="t">{item.title}</span></span>
                </button>
                {@render heart(item.path)}
                <button class="mini" aria-label={t("removeFromQueue")} title={t("removeFromQueue")} onclick={() => invoke("queue_remove", { index: i })}>×</button>
              </div>
            {/each}
          {:else}
            <p class="empty">{t("queueEmpty")}</p>
          {/if}
        {:else if tab === "playlists"}
          {#if naming}
            <form
              class="naming"
              onsubmit={(e) => {
                e.preventDefault();
                saveName();
              }}
            >
              <!-- svelte-ignore a11y_autofocus -->
              <input bind:value={naming.name} placeholder={t("playlistName")} autofocus spellcheck="false" />
              <button class="pill on" type="submit">{t("save")}</button>
            </form>
          {/if}
          {#if !openList}
            <div class="grid">
              {#each lists as l (l.id)}
                <div class="tile">
                  <button class="art list-art" class:fav={l.id === "favorites"} onclick={() => showList(l)} aria-label={listName(l)}>
                    {#if l.id === "favorites"}
                      <span class="big-heart">♥</span>
                    {:else if listCovers[`${l.id}:${l.count}`]}
                      <img src={listCovers[`${l.id}:${l.count}`]} alt="" draggable="false" />
                    {:else}
                      <svg viewBox="0 0 24 24"><path d="M4 6h16M4 12h10M4 18h7" /><circle cx="18" cy="17" r="2.5" /><path d="M20.5 17V10" /></svg>
                    {/if}
                  </button>
                  {#if l.count}
                    <button class="tile-play" aria-label={t("playPlaylist")} onclick={() => playList(l)}>
                      <svg viewBox="0 0 24 24"><path d="M8.5 5.6v12.8a1 1 0 0 0 1.5.86l10.2-6.4a1 1 0 0 0 0-1.72L10 4.74A1 1 0 0 0 8.5 5.6z" /></svg>
                    </button>
                  {/if}
                  <span class="tile-name" title={listName(l)}>{listName(l)}</span>
                  <span class="tile-sub">{t("trackCount", { count: l.count })}</span>
                </div>
              {/each}
              {#if !naming}
                <button class="tile add" onclick={() => (naming = { id: null, name: "", paths: [] })}>
                  <span class="art"><svg viewBox="0 0 24 24"><path d="M12 6v12M6 12h12" /></svg></span>
                  <span class="tile-name">{t("newPlaylist")}</span>
                </button>
              {/if}
            </div>
            <p class="hint">{t("playlistTip")}</p>
          {:else}
            <div class="toolbar">
              {#if listTracks.length}
                <button class="pill on" onclick={() => invoke("play_tracks", { paths: listTracks.map((t) => t.path), start: 0 })}>▶ {t("play")}</button>
              {/if}
              {#if openList.id !== "favorites"}
                <button class="pill" onclick={() => (naming = { id: openList!.id, name: openList!.name, paths: [] })}>{t("rename")}</button>
                <button class="pill" onclick={() => deleteList(openList!)}>{t("delete")}</button>
              {/if}
            </div>
            {#each listTracks as t, i (t.path)}{@render trackRow(t, i, true)}{/each}
            {#if listTracks.length === 0}
              <p class="empty">
                {openList.id === "favorites" ? t("favoritesEmpty") : t("playlistEmpty")}
              </p>
            {/if}
          {/if}
        {:else if results}
          {#each results as t, i (t.path)}{@render trackRow(t, i, false)}{/each}
          {#if results.length === 0}<p class="empty">{t("nothingFound")}</p>{/if}
        {:else if !view}
          <div class="grid">
            {#each roots as f (f.path)}{@render tile(f, true)}{/each}
            <button class="tile add" onclick={() => addFolder()}>
              <span class="art"><svg viewBox="0 0 24 24"><path d="M12 6v12M6 12h12" /></svg></span>
              <span class="tile-name">{t("addFolder")}</span>
            </button>
          </div>
          {#if roots.length === 0}
            <p class="empty">{t("addFolderHint")}</p>
          {/if}
        {:else}
          {#if view.folders.length}
            <div class="grid">
              {#each view.folders as f (f.path)}{@render tile(f, false)}{/each}
            </div>
          {/if}
          {#if view.tracks.length}
            <div class="toolbar">
              <button class="pill on" onclick={() => playFolder(view!.path)}>▶ {t("playAll")}</button>
              <button class="pill" onclick={(e) => openMenu(e, [], { path: view!.path, name: view!.name }, true)}>{t("addToPlaylist")}</button>
              <button class="pill icon-only" aria-label={t("more")} title={t("more")} onclick={(e) => openMenu(e, [], { path: view!.path, name: view!.name })}>⋯</button>
            </div>
            {#each view.tracks as t, i (t.path)}{@render trackRow(t, i, false)}{/each}
          {/if}
        {/if}
      </div>

      {#if menu}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="menu-shade" onclick={() => (menu = null)}></div>
        <div class="menu" style:left="{menu.x}px" style:top="{menu.y}px">
          {#if !menu.lists}
            {#if menu.folder}
              <button
                onclick={() => {
                  playFolder(menu!.folder!.path);
                  menu = null;
                }}>{t("playFolder")}</button
              >
            {/if}
            <button onclick={() => queueAction("next")}>{t("playNext")}</button>
            <button onclick={() => queueAction("add")}>{t("addToQueue")}</button>
            <button onclick={() => (menu = { ...menu!, lists: true })}>{t("addToPlaylistMenu")}</button>
          {:else}
            {#if menu.folder}
              <button class="new" onclick={() => newListFromFolder(menu!.folder!)}>{t("newFromFolder", { name: menu.folder.name })}</button>
            {/if}
            {#each lists.filter((l) => l.id !== "favorites") as l (l.id)}
              <button onclick={() => addToList(l)}>{l.name}</button>
            {/each}
            {#if !menu.folder}
              <button
                class="new"
                onclick={() => {
                  naming = { id: null, name: "", paths: menu!.paths };
                  menu = null;
                  tab = "playlists";
                  openList = null;
                }}>{t("newPlaylistMenu")}</button
              >
            {/if}
          {/if}
        </div>
      {/if}

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
  .settings-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  /* The developer's mark, the same on every product: "by Zhichkin". */
  .signature {
    display: inline-flex;
    align-items: baseline;
    gap: 5px;
    text-shadow: 0 1px 4px rgb(0 0 0 / 0.5);
  }
  .signature .by {
    font-size: 10px;
    font-style: italic;
    color: rgb(255 255 255 / 0.45);
  }
  .signature .author {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    background: linear-gradient(90deg, #f3d9ff, #9fb8ff);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
    /* text-shadow would paint over the clipped gradient */
    text-shadow: none;
    filter: drop-shadow(0 1px 3px rgb(0 0 0 / 0.5));
  }
  .done {
    width: auto;
    height: 24px;
    padding: 0 12px;
    border-radius: 12px;
    background: rgb(255 255 255 / 0.14);
  }
  .lang {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: 11px;
    background: rgb(255 255 255 / 0.08);
  }
  .lang button {
    width: auto;
    height: 18px;
    padding: 0 6px;
    border-radius: 9px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.04em;
    color: rgb(255 255 255 / 0.6);
  }
  .lang button.on {
    background: rgb(255 255 255 / 0.9);
    color: #10121a;
  }

  /* ---- library panel ---- */
  .panel {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-top: 1px solid rgb(255 255 255 / 0.1);
    /* Lists need more backing than the compact card: readable on bright, busy wallpapers. */
    background: rgb(10 12 20 / 0.42);
  }
  .widget.up .panel {
    border-top: 0;
    border-bottom: 1px solid rgb(255 255 255 / 0.1);
  }
  /* Two rows: the title (it can be long) on top, the tabs under it, so neither covers the other. */
  header {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 12px 8px 16px;
  }
  .head-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
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
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 2px;
    padding: 2px;
    border-radius: 14px;
    background: rgb(255 255 255 / 0.08);
  }
  .tabs button {
    width: 100%;
    height: 24px;
    padding: 0 6px;
    border-radius: 12px;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
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
    text-overflow: ellipsis;
    user-select: text;
  }
  .search input::placeholder {
    color: rgb(255 255 255 / 0.45);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-x: hidden;
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
    overflow-wrap: anywhere;
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
  .tile-more {
    position: absolute;
    right: 40px;
    top: 6px;
    width: 30px;
    height: 30px;
    font-size: 15px;
    line-height: 1;
    background: rgb(0 0 0 / 0.55);
    opacity: 0;
  }
  .tile:hover .tile-more {
    opacity: 1;
  }
  .tile-sub {
    margin-top: -4px;
    font-size: 11px;
    color: rgb(255 255 255 / 0.55);
    text-shadow: 0 1px 4px rgb(0 0 0 / 0.5);
  }
  .list-art.fav {
    background: linear-gradient(135deg, #ff6b8a, #7b5cff);
  }
  .big-heart {
    font-size: 40px;
    line-height: 1;
    color: #fff;
    text-shadow: 0 4px 14px rgb(0 0 0 / 0.35);
  }
  .hint {
    margin: 2px 2px 8px;
    font-size: 11px;
    color: rgb(255 255 255 / 0.5);
    text-shadow: 0 1px 4px rgb(0 0 0 / 0.5);
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


  .row {
    position: relative;
    width: 100%;
    height: 40px;
    padding: 0 4px 0 0;
    border-radius: 8px;
    display: flex;
    align-items: center;
    gap: 2px;
    text-align: left;
    transition: background 0.15s;
  }
  .row:hover {
    background: rgb(255 255 255 / 0.07);
  }
  .row.current {
    background: rgb(255 255 255 / 0.12);
  }
  .row.over {
    box-shadow: inset 0 2px 0 rgb(255 255 255 / 0.8);
  }
  .row-main {
    flex: 1;
    min-width: 0;
    width: auto;
    height: 100%;
    border-radius: 8px;
    display: flex;
    align-items: center;
    gap: 8px;
    text-align: left;
  }
  .row-main:hover,
  .row-main:active {
    background: transparent;
    transform: none;
  }
  .grip {
    flex: none;
    width: 14px;
    font-size: 11px;
    letter-spacing: -2px;
    color: rgb(255 255 255 / 0.35);
    cursor: grab;
    touch-action: none;
  }
  .mini {
    flex: none;
    width: 24px;
    height: 24px;
    font-size: 15px;
    line-height: 1;
    opacity: 0;
  }
  .row:hover .mini,
  .mini.heart.on {
    opacity: 1;
  }
  .mini.heart svg {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linejoin: round;
  }
  .mini.heart.on {
    color: #ff6b8a;
  }
  .mini.heart.on svg {
    fill: currentColor;
  }

  /* Action buttons share one height and wrap instead of overlapping on a narrow panel. */
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin: 0 0 8px;
  }
  .toolbar .count {
    margin-left: auto;
    font-size: 11px;
    color: rgb(255 255 255 / 0.5);
    white-space: nowrap;
  }
  .pill {
    flex: none;
    width: auto;
    height: 28px;
    padding: 0 12px;
    border-radius: 14px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    font-size: 12px;
    white-space: nowrap;
    background: rgb(255 255 255 / 0.08);
  }
  .pill:hover {
    background: rgb(255 255 255 / 0.16);
  }
  .pill:active {
    background: rgb(255 255 255 / 0.22);
    transform: scale(0.96);
  }
  .pill:focus-visible {
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.5);
  }
  .pill.icon-only {
    width: 28px;
    padding: 0;
    font-size: 15px;
    line-height: 1;
  }
  .pill svg {
    width: 13px;
    height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .pill.on {
    background: rgb(255 255 255 / 0.9);
    color: #10121a;
  }
  .pill.on:hover {
    background: #fff;
    color: #10121a;
  }
  .pill.on:active {
    background: rgb(255 255 255 / 0.8);
    color: #10121a;
  }


  .naming {
    display: flex;
    gap: 6px;
    margin: 0 0 10px;
  }
  .naming input {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 10px;
    border: 0;
    outline: 0;
    border-radius: 8px;
    background: rgb(255 255 255 / 0.1);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.25);
    color: #fff;
    font: inherit;
    font-size: 13px;
    user-select: text;
  }

  .menu-shade {
    position: absolute;
    inset: 0;
    z-index: 5;
  }
  .menu {
    position: absolute;
    z-index: 6;
    width: 184px;
    max-height: 190px;
    overflow-y: auto;
    padding: 4px;
    border-radius: 10px;
    background: rgb(22 24 34 / 0.97);
    box-shadow:
      0 10px 30px rgb(0 0 0 / 0.5),
      inset 0 0 0 1px rgb(255 255 255 / 0.1);
  }
  .menu button {
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border-radius: 6px;
    display: flex;
    align-items: center;
    font-size: 12px;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .menu button:active {
    transform: none;
  }
  .menu .new {
    color: rgb(255 255 255 / 0.65);
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
