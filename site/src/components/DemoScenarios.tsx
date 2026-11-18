import { useEffect, useState } from "react";
import { Segmented, type Segment } from "../../../app/src/components/Segmented";

type Scenario = "photo" | "slide" | "chart";

const SCENARIOS: Segment<Scenario>[] = [
  { value: "photo", label: "Describe a photo" },
  { value: "slide", label: "Search inside slides" },
  { value: "chart", label: "Find a chart’s code" },
];

const demo = () => document.querySelector<HTMLIFrameElement>("[data-demo] iframe");

export default function DemoScenarios() {
  const [scenario, setScenario] = useState<Scenario>("photo");

  useEffect(() => {
    const heard = (event: MessageEvent) => {
      if (event.origin !== location.origin || event.source !== demo()?.contentWindow || event.data?.type !== "mesa-demo-playing") return;
      if (SCENARIOS.some((one) => one.value === event.data.scenario)) setScenario(event.data.scenario);
    };
    window.addEventListener("message", heard);
    return () => window.removeEventListener("message", heard);
  }, []);

  const choose = (chosen: Scenario) => {
    if (chosen === scenario) return;
    setScenario(chosen);
    demo()?.contentWindow?.postMessage({ type: "mesa-demo-scenario", scenario: chosen }, location.origin);
  };

  return (
    <>
      <span id="demo-scenarios-label" hidden>
        What the demo shows
      </span>
      <Segmented segments={SCENARIOS} value={scenario} onChange={choose} labelledBy="demo-scenarios-label" />
    </>
  );
}
