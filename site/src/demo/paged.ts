const PAGED = /^(.*\/demo\/files\/[^/?#]+\/)(?:\?([^#]*))?$/;

export function pagePicture(url: string): string {
  const found = PAGED.exec(url);
  if (!found) return url;
  const page = Number(new URLSearchParams(found[2] ?? "").get("page") ?? "1");
  return `${found[1]}${Number.isInteger(page) && page >= 1 ? page : 1}.svg`;
}

const source = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, "src")!;
Object.defineProperty(HTMLImageElement.prototype, "src", {
  ...source,
  set(this: HTMLImageElement, url: string) {
    source.set!.call(this, pagePicture(String(url)));
  },
});

const setAttribute = Element.prototype.setAttribute;
Element.prototype.setAttribute = function (this: Element, name: string, value: string) {
  setAttribute.call(this, name, this instanceof HTMLImageElement && name.toLowerCase() === "src" ? pagePicture(String(value)) : value);
};
