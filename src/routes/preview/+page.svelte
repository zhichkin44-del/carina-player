<script lang="ts">
  // Dev-only stand: the widget with fake data on a real wallpaper, to check the UI in a browser
  // without touching the desktop. Production builds render nothing here.
  import { onMount } from "svelte";

  const dev = import.meta.env.DEV;
  let Widget = $state<any>(null);

  const cover = (a: string, b: string) =>
    "data:image/svg+xml," +
    encodeURIComponent(
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><defs><linearGradient id="g" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="10" height="10" fill="url(#g)"/><circle cx="5" cy="5" r="2.6" fill="none" stroke="white" stroke-width=".5"/></svg>`,
    );
  const track = (n: number, title: string, artist: string, dir: string) => ({
    path: `${dir}/${n}.mp3`,
    title,
    artist,
    album: "",
    number: n,
    duration: 120 + n * 7,
  });
  const albums = [
    { path: "/m/ALBLAK 52", name: "03_ALBLAK_52" },
    { path: "/m/Ken Carson", name: "12_Ken_Carson" },
    { path: "/m/Memphis", name: "01_Memphis_Dark_Atlanta" },
  ];
  const data: Record<string, (a: any) => any> = {
    set_panel: () => "down",
    library_roots: () => [{ path: "/m", name: "Aggressive_Trap_17_Legends" }],
    library_open: ({ path }) => ({
      path,
      name: path === "/m" ? "Aggressive_Trap_17_Legends" : path.split("/").pop(),
      folders: path === "/m" ? albums : [],
      tracks: path === "/m" ? [] : [1, 2, 3, 4, 5].map((n) => track(n, `Трек ${n}`, "ALBLAK 52", path)),
    }),
    library_cover: ({ path }) => (path.includes("Ken") ? cover("#e07a3a", "#3a1a10") : path.includes("Memphis") ? cover("#2d6cdf", "#0b1633") : null),
    playlists: () => [
      { id: "favorites", name: "Избранное", count: 7 },
      { id: "p1", name: "Night mix", count: 12 },
      { id: "p2", name: "Gym", count: 0 },
    ],
    playlist_cover: ({ id }) => (id === "p1" ? cover("#9b4dff", "#1b0d3a") : null),
    favorites: () => [],
    playlist_tracks: () => [],
    folder_tracks: () => ["/m/a.mp3", "/m/b.mp3"],
  };

  onMount(async () => {
    if (!dev) return;
    const { mockIPC, mockWindows } = await import("@tauri-apps/api/mocks");
    mockWindows("widget");
    mockIPC((cmd, args) => (cmd in data ? data[cmd](args) : null));
    document.documentElement.style.background = "url(/@fs/C:/carina/design/preview-bg.webp) -500px -330px / 1920px auto";
    Widget = (await import("../+page.svelte")).default;
  });
</script>

{#if dev && Widget}
  <Widget />
{/if}
