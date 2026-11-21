<script lang="ts">
  // Dev-only stand: the widget with fake data on a real wallpaper, to check the UI in a browser
  // without touching the desktop. Production builds render nothing here.
  import { onMount } from "svelte";
  import previewBg from "../../../design/preview-bg.webp";

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
  const allRoots = [
    { path: "/m", name: "Demo_Library" },
    { path: "/m/02_Dark_Jazz_Noir_Vocals_Remastered_Deluxe_Edition_2024_Mix", name: "02_Dark_Jazz_Noir_Vocals_Remastered_Deluxe_Edition_2024_Mix" },
    { path: "/m/Memphis", name: "01_Memphis_Dark_Atlanta" },
    { path: "/m/Ken Carson", name: "12_Ken_Carson" },
    { path: "/m/ALBLAK 52", name: "03_ALBLAK_52" },
    { path: "/m/Horror", name: "Horror" },
    { path: "/m/Lofi", name: "Lofi_Study" },
    { path: "/m/Chill", name: "Chill_Beats" },
    { path: "/m/Drill", name: "Drill_Mix" },
    { path: "/m/Phonk", name: "Phonk_Night" },
    { path: "/m/Synth", name: "Synthwave_Road" },
    { path: "/m/Rain", name: "Rain_Sounds" },
  ];
  // Seven folders by default (the owner's case: three rows with the "+" card); screenshots can ask for more.
  let rootCount = 7;
  const roots = () => allRoots.slice(0, rootCount);
  const queue = {
    items: [1, 2, 3, 4, 5, 6, 7, 8].map((n) => ({ path: `/q/${n}.mp3`, title: `Night Drive Part ${n}` })),
    index: 2,
    shuffle: false,
    repeat: "all",
  };
  const data: Record<string, (a: any) => any> = {
    set_panel: () => "down",
    library_roots: () => roots(),
    library_open: ({ path }) => ({
      path,
      name: path === "/m" ? "Demo_Library" : path.split("/").pop(),
      folders: path === "/m" ? albums : [],
      tracks: path === "/m" ? [] : [1, 2, 3, 4, 5].map((n) => track(n, `Track ${n}`, "ALBLAK 52", path)),
    }),
    library_cover: ({ path }) => (path.includes("Ken") ? cover("#e07a3a", "#3a1a10") : path.includes("Memphis") ? cover("#2d6cdf", "#0b1633") : null),
    playlists: () => [
      { id: "favorites", name: "Favorites", count: 7 },
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
    const { emit } = await import("@tauri-apps/api/event");
    mockWindows("widget");
    mockIPC((cmd, args) => (cmd in data ? data[cmd](args) : null), { shouldMockEvents: true });
    // Screenshots and checks ask for the queue through the page: the widget receives it as a "queue" event.
    (window as any).__emitQueue = () => emit("queue", queue);
    (window as any).__setRootCount = (n: number) => (rootCount = n);
    document.documentElement.style.background = `url(${previewBg}) -500px -330px / 1920px auto`;
    Widget = (await import("../+page.svelte")).default;
  });
</script>

{#if dev && Widget}
  <Widget />
{/if}
