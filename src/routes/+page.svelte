<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
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

  type Glass = { opacity: number; blur: boolean };
  const GLASS_KEY = "carina.glass";

  function loadGlass(): Glass {
    try {
      const saved = JSON.parse(localStorage.getItem(GLASS_KEY) ?? "");
      if (typeof saved?.opacity === "number" && typeof saved?.blur === "boolean") return saved;
    } catch {}
    return { opacity: 25, blur: true };
  }

  let glass = $state<Glass>(loadGlass());
  let settingsOpen = $state(false);

  // Blur is native (Windows); the background opacity is painted by the page over it.
  $effect(() => {
    invoke("set_glass", { blur: glass.blur });
  });
  $effect(() => {
    try {
      localStorage.setItem(GLASS_KEY, JSON.stringify(glass));
    } catch {}
  });

  let track = $state<NowPlaying | null>(null);
  let position = $state(0);
  let receivedAt = 0;

  const progress = $derived(
    track && track.duration > 0 ? Math.min(1, position / track.duration) : 0,
  );

  function clock(seconds: number) {
    const s = Math.max(0, Math.floor(seconds));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }

  onMount(() => {
    const unlisten = listen<NowPlaying>("now-playing", (e) => {
      track = e.payload.title || e.payload.artist ? e.payload : null;
      position = e.payload.position;
      receivedAt = performance.now();
    });

    // Sessions report position rarely; move the bar smoothly in between.
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
      unlisten.then((f) => f());
    };
  });
</script>

<main
  class="widget"
  style:--glass={glass.opacity / 100}
  data-tauri-drag-region
>
  <button
    class="gear"
    class:active={settingsOpen}
    aria-label="Настройки"
    onclick={() => (settingsOpen = !settingsOpen)}
  >
    <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3" /><path d="M12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1 7 17M17 7l2.1-2.1" /></svg>
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
      <label class="row">
        <span>Фон</span>
        <input type="range" min="0" max="100" step="1" bind:value={glass.opacity} />
        <span class="value">{glass.opacity}%</span>
      </label>
      <label class="row">
        <span>Размытие</span>
        <input type="checkbox" class="switch" bind:checked={glass.blur} />
      </label>
      <button class="done" onclick={() => (settingsOpen = false)}>Готово</button>
    </section>
  {:else}
  <section class="info" data-tauri-drag-region>
    <div class="text" data-tauri-drag-region>
      <div class="title" data-tauri-drag-region>{track?.title || "Тишина"}</div>
      <div class="artist" data-tauri-drag-region>
        {track ? track.artist || track.album || "Неизвестный исполнитель" : "Включи музыку в любом плеере"}
      </div>
    </div>

    <div class="bottom" data-tauri-drag-region>
      <nav class="controls">
        <button
          class="skip"
          aria-label="Назад"
          disabled={!track?.canPrevious}
          onclick={() => invoke("media_previous")}
        >
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
          title={track && !track.canNext ? "Плеер не даёт переключить: в его очереди нет следующего трека" : undefined}
          onclick={() => invoke("media_next")}
        >
          <svg viewBox="0 0 24 24"><rect x="16.4" y="5.5" width="2.6" height="13" rx="1.2" /><path d="M5 6.6v10.8a1 1 0 0 0 1.55.83L14.4 13a1.2 1.2 0 0 0 0-2L6.55 5.77A1 1 0 0 0 5 6.6z" /></svg>
        </button>
      </nav>
      <span class="time" class:hidden={!track || track.duration <= 0} data-tauri-drag-region>
        {clock(position)} <i>/</i> {clock(track?.duration ?? 0)}
      </span>
    </div>
  </section>
  {/if}

  <div class="progress" class:hidden={!track || track.duration <= 0}>
    <div class="fill" style:width="{progress * 100}%"></div>
  </div>
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

  .widget {
    --glass: 0.25;
    position: relative;
    box-sizing: border-box;
    height: 100vh;
    display: flex;
    gap: 16px;
    padding: 16px;
    border-radius: 8px;
    overflow: hidden;
    /* Constant sheen: the opacity setting only darkens the tint, never lightens the glass. */
    background: linear-gradient(135deg, rgb(255 255 255 / 0.05), rgb(255 255 255 / 0));
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.12);
  }
  /* Background opacity: smoked tint over the (optionally blurred) wallpaper. */
  .widget {
    background-color: rgb(14 16 24 / calc(0.88 * var(--glass)));
  }

  .gear {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 22px;
    height: 22px;
    opacity: 0;
    transition: opacity 0.2s;
  }
  .widget:hover .gear,
  .gear.active {
    opacity: 1;
  }
  .gear svg {
    width: 13px;
    height: 13px;
    fill: none;
  }

  .settings {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 10px;
    padding-right: 18px;
    font-size: 12px;
    color: rgb(255 255 255 / 0.8);
  }
  .settings .row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .settings .row > span:first-child {
    width: 62px;
  }
  .settings .value {
    width: 34px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  input[type="range"] {
    flex: 1;
    min-width: 0;
    accent-color: #fff;
  }
  .switch {
    appearance: none;
    width: 30px;
    height: 16px;
    margin: 0;
    border-radius: 8px;
    background: rgb(255 255 255 / 0.2);
    position: relative;
    transition: background 0.15s;
  }
  .switch::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #fff;
    transition: transform 0.15s;
  }
  .switch:checked {
    background: rgb(255 255 255 / 0.55);
  }
  .switch:checked::after {
    transform: translateX(14px);
  }
  .done {
    align-self: flex-end;
    width: auto;
    height: 24px;
    padding: 0 12px;
    border-radius: 12px;
    font: inherit;
    background: rgb(255 255 255 / 0.14);
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

  .title,
  .artist {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-shadow: 0 1px 6px rgb(0 0 0 / 0.35);
  }
  .title {
    font-size: 15px;
    font-weight: 600;
    letter-spacing: 0.01em;
  }
  .artist {
    margin-top: 2px;
    font-size: 13px;
    color: rgb(255 255 255 / 0.7);
  }

  .text {
    padding-right: 18px;
  }

  .bottom {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .time {
    margin-left: auto;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: rgb(255 255 255 / 0.55);
    white-space: nowrap;
  }
  .time i {
    font-style: normal;
    opacity: 0.5;
  }
  .hidden {
    visibility: hidden;
  }

  /* Hairline progress along the bottom edge of the glass. */
  .progress {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 2px;
    background: rgb(255 255 255 / 0.1);
  }
  .progress .fill {
    height: 100%;
    background: linear-gradient(90deg, rgb(255 255 255 / 0.55), #fff);
    box-shadow: 0 0 8px rgb(255 255 255 / 0.6);
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
    transition:
      background 0.15s,
      color 0.15s,
      transform 0.12s,
      box-shadow 0.2s;
  }
  button:hover {
    background: rgb(255 255 255 / 0.1);
    color: #fff;
  }
  button:active {
    transform: scale(0.9);
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
  .gear svg {
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
  }
</style>
