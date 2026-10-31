import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export default function App() {
  const [name, setName] = useState("world");
  const [greeting, setGreeting] = useState("");
  const [ticks, setTicks] = useState<number[]>([]);

  async function sayHello() {
    setGreeting(await invoke<string>("greet", { name }));
  }

  useEffect(() => {
    const unlisten = listen<number>("tick", (e) => setTicks((t) => [...t, e.payload]));
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  async function startCounting() {
    setTicks([]);
    await invoke("start_counting", { to: 5 });
  }

  return (
    <main style={{ fontFamily: "sans-serif", padding: 24 }}>
      <h1>Hello Tauri</h1>

      <h2>1. Command (React asks, Rust answers)</h2>
      <input value={name} onChange={(e) => setName(e.target.value)} />
      <button onClick={sayHello}>Greet</button>
      <p>{greeting}</p>

      <h2>2. Event (Rust pushes, React listens)</h2>
      <button onClick={startCounting}>Count to 5 in Rust</button>
      <p>Received: {ticks.join(", ")}</p>
    </main>
  );
}
