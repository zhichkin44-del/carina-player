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
  };

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

<main class="widget" data-tauri-drag-region>
  <div class="cover" data-tauri-drag-region>
    {#if track?.cover}
      <img src={track.cover} alt="" draggable="false" />
    {:else}
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 18V5l12-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="16" r="3" /></svg>
    {/if}
  </div>

  <section class="info" data-tauri-drag-region>
    <div class="text" data-tauri-drag-region>
      <div class="title" data-tauri-drag-region>{track?.title || "Тишина"}</div>
      <div class="artist" data-tauri-drag-region>
        {track ? track.artist || track.album || "Неизвестный исполнитель" : "Включи музыку в любом плеере"}
      </div>
    </div>

    <div class="timeline" class:hidden={!track || track.duration <= 0} data-tauri-drag-region>
      <span>{clock(position)}</span>
      <div class="bar"><div class="fill" style:width="{progress * 100}%"></div></div>
      <span>{clock(track?.duration ?? 0)}</span>
    </div>

    <nav class="controls">
      <button aria-label="Назад" onclick={() => invoke("media_previous")}>
        <svg viewBox="0 0 24 24"><path d="M6 5v14" /><path d="M19 5 9 12l10 7z" /></svg>
      </button>
      <button class="play" aria-label={track?.playing ? "Пауза" : "Играть"} onclick={() => invoke("media_toggle")}>
        {#if track?.playing}
          <svg viewBox="0 0 24 24"><path d="M8 5v14M16 5v14" /></svg>
        {:else}
          <svg viewBox="0 0 24 24"><path d="M7 4.5v15L19.5 12z" /></svg>
        {/if}
      </button>
      <button aria-label="Вперёд" onclick={() => invoke("media_next")}>
        <svg viewBox="0 0 24 24"><path d="M18 5v14" /><path d="M5 5l10 7-10 7z" /></svg>
      </button>
    </nav>
  </section>
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
    box-sizing: border-box;
    height: 100vh;
    display: flex;
    gap: 16px;
    padding: 16px;
    border-radius: 8px;
    background: linear-gradient(135deg, rgb(255 255 255 / 0.1), rgb(255 255 255 / 0.02));
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.12);
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

  .timeline {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: rgb(255 255 255 / 0.6);
  }
  .timeline.hidden {
    visibility: hidden;
  }
  .bar {
    flex: 1;
    height: 3px;
    border-radius: 2px;
    background: rgb(255 255 255 / 0.18);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 2px;
    background: rgb(255 255 255 / 0.9);
  }

  .controls {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 14px;
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
    color: rgb(255 255 255 / 0.85);
    transition:
      background 0.15s,
      transform 0.1s;
  }
  button:hover {
    background: rgb(255 255 255 / 0.12);
    color: #fff;
  }
  button:active {
    transform: scale(0.92);
  }
  button.play {
    width: 36px;
    height: 36px;
    background: rgb(255 255 255 / 0.16);
  }
  button.play:hover {
    background: rgb(255 255 255 / 0.26);
  }
  button svg {
    width: 16px;
    height: 16px;
    fill: currentColor;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  button.play svg {
    width: 18px;
    height: 18px;
  }
</style>
