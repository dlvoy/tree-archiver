/**
 * Inline SVG glyphs, drawn on a 16px grid with a single stroke weight so the
 * row reads as one drafted line rather than a strip of stickers.
 */
import type { ReactNode } from "react";
import type { NodeKind } from "../api/commands";

type Category =
  | "folder"
  | "folderOpen"
  | "folderSpine"
  | "files"
  | "sources"
  | "code"
  | "text"
  | "image"
  | "audio"
  | "video"
  | "archive"
  | "binary";

const BY_EXT: Record<string, Category> = {};
const register = (cat: Category, exts: string[]) => {
  for (const e of exts) BY_EXT[e] = cat;
};

register("code", [
  "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "c", "h", "cpp", "hpp",
  "cs", "rb", "php", "sh", "ps1", "sql", "html", "css", "scss", "json", "toml",
  "yaml", "yml", "xml", "kt", "swift", "lua", "vue", "svelte",
]);
register("text", ["txt", "md", "rst", "log", "csv", "pdf", "doc", "docx", "rtf", "odt"]);
register("image", ["png", "jpg", "jpeg", "gif", "bmp", "svg", "webp", "ico", "tif", "tiff", "psd"]);
register("audio", ["mp3", "wav", "flac", "ogg", "m4a", "aac", "wma"]);
register("video", ["mp4", "mkv", "avi", "mov", "wmv", "webm", "flv", "m4v"]);
register("archive", ["zip", "tar", "gz", "bz2", "xz", "7z", "rar", "iso", "cab", "zst"]);
register("binary", ["exe", "dll", "so", "dylib", "bin", "obj", "o", "lib", "pdb", "msi"]);

function categorize(
  kind: NodeKind,
  ext: string | null,
  spine: boolean,
  open: boolean,
): Category {
  if (kind === "filesGroup") return "files";
  if (kind === "syntheticRoot") return "sources";
  if (kind === "dir") {
    if (spine) return "folderSpine";
    return open ? "folderOpen" : "folder";
  }
  return (ext && BY_EXT[ext]) || "text";
}

/** Shared page outline for the document-ish categories. */
const PAGE = "M4 1.5h5l3 3v10h-8z";

const PATHS: Record<Category, ReactNode> = {
  folder: (
    <path d="M1.5 3.5h4.5l1.5 2h7v8h-13z" />
  ),
  folderOpen: (
    <>
      <path d="M1.5 3.5h4.5l1.5 2h7v2" />
      <path d="M1.5 13.5l1.8-6h12.2l-1.8 6z" />
    </>
  ),
  // A dashed outline: this folder is on the path to a source, not itself added.
  folderSpine: (
    <path d="M1.5 3.5h4.5l1.5 2h7v8h-13z" strokeDasharray="2.5 2" />
  ),
  // The <files> group: stacked sheets.
  files: (
    <>
      <path d="M2.5 4.5h6l2 2v7h-8z" />
      <path d="M5.5 2.5h5l2.5 2.5v6" />
    </>
  ),
  sources: (
    <>
      <path d="M8 1.5v13" />
      <path d="M8 5h5.5M8 9h5.5M8 13h5.5M8 5H2.5M8 9H2.5" />
    </>
  ),
  code: (
    <>
      <path d={PAGE} />
      <path d="M6.5 8l-1.5 1.75 1.5 1.75M9.5 8l1.5 1.75-1.5 1.75" />
    </>
  ),
  text: (
    <>
      <path d={PAGE} />
      <path d="M6 8h4M6 10.5h4" />
    </>
  ),
  image: (
    <>
      <path d={PAGE} />
      <path d="M5 12.5l2.5-3 1.5 1.75 1-1.25 2 2.5" />
    </>
  ),
  audio: (
    <>
      <path d={PAGE} />
      <path d="M6 12V8.5l3.5-.75v3.5" />
      <circle cx="5.4" cy="12.2" r="0.9" />
      <circle cx="8.9" cy="11.5" r="0.9" />
    </>
  ),
  video: (
    <>
      <path d={PAGE} />
      <path d="M6.5 8.5l3.5 2-3.5 2z" />
    </>
  ),
  archive: (
    <>
      <path d={PAGE} />
      <path d="M7 5.5h2M7 7.5h2M7 9.5h2" />
      <path d="M6.75 11.5h2.5v2.5h-2.5z" />
    </>
  ),
  binary: (
    <>
      <path d={PAGE} />
      <path d="M5.5 8.5h2v3h-2zM8.5 8.5h2v3h-2z" />
    </>
  ),
};

export function FileIcon({
  kind,
  ext,
  spine,
  open,
}: {
  kind: NodeKind;
  ext: string | null;
  spine: boolean;
  open: boolean;
}) {
  const cat = categorize(kind, ext, spine, open);
  return (
    <svg
      className={`icon icon--${cat}`}
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.25"
      strokeLinejoin="round"
      strokeLinecap="round"
      aria-hidden="true"
    >
      {PATHS[cat]}
    </svg>
  );
}
