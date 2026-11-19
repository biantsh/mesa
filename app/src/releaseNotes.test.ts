import { describe, expect, it } from "vitest";
import { readNotes } from "./releaseNotes";

describe("readNotes", () => {
  it("parses a changelog section", () => {
    expect(
      readNotes(`### New

- Search finds **more** files.
- Fixed: previews of \`.heic\` photos
  were blank.

Thanks to [everyone](https://example.com) who reported it.`),
    ).toEqual([
      { kind: "heading", inlines: [{ kind: "text", text: "New" }] },
      {
        kind: "list",
        items: [
          [
            { kind: "text", text: "Search finds " },
            { kind: "bold", text: "more" },
            { kind: "text", text: " files." },
          ],
          [
            { kind: "text", text: "Fixed: previews of " },
            { kind: "code", text: ".heic" },
            { kind: "text", text: " photos were blank." },
          ],
        ],
      },
      { kind: "paragraph", inlines: [{ kind: "text", text: "Thanks to everyone who reported it." }] },
    ]);
  });

  it("don't use lookbehind (unsupported before Safari 16.4)", async () => {
    const source = (await import("./releaseNotes.ts?raw")).default as string;
    expect(source).not.toMatch(/\(\?<[!=]/);
  });
});
