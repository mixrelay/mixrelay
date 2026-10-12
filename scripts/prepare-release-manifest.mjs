import { lstatSync, statSync, writeFileSync } from "node:fs";
import { extname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

function validTimestamp(value) {
  const match = value.match(/^(\d{4}-\d{2}-\d{2})T(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/);
  if (!match || !Number.isFinite(Date.parse(value))) return false;
  const day = new Date(`${match[1]}T00:00:00Z`);
  return Number.isFinite(day.getTime()) && day.toISOString().slice(0, 10) === match[1];
}

/** Writes only a new manifest for explicitly selected, existing final installers. */
export function prepareReleaseManifest(args) {
  const { values } = parseArgs({
    args,
    options: {
      directory: { type: "string" },
      product: { type: "string" },
      version: { type: "string" },
      "published-at": { type: "string" },
      "notes-url": { type: "string" },
      artifact: { type: "string", multiple: true, default: [] },
    },
  });
  for (const name of ["directory", "product", "version", "published-at"]) {
    if (!values[name]) throw new Error(`Missing required option: --${name}`);
  }
  if (values.product !== "mix-relay") {
    throw new Error("--product must be mix-relay");
  }
  if (!/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(values.version)) {
    throw new Error("--version must be canonical stable MAJOR.MINOR.PATCH");
  }
  if (!validTimestamp(values["published-at"])) {
    throw new Error("--published-at must be a valid RFC3339 timestamp with a timezone");
  }
  if (values["notes-url"] !== undefined) {
    const url = new URL(values["notes-url"]);
    if (!values["notes-url"].startsWith("https://") || url.protocol !== "https:"
      || !url.hostname || url.username || url.password || url.hash) {
      throw new Error("--notes-url must be a public HTTPS URL without credentials or fragment");
    }
  }
  if (values.artifact.length === 0) throw new Error("At least one --artifact OS:ARCH:FILENAME is required");
  const directory = resolve(values.directory);
  if (!statSync(directory).isDirectory()) throw new Error("--directory must be an existing directory");
  const seen = new Set();
  const artifacts = values.artifact.map(value => {
    const parts = value.split(":");
    if (parts.length !== 3) throw new Error("--artifact must be OS:ARCH:FILENAME");
    const [os, arch, file] = parts;
    if (!file || file === "." || file === ".." || /[/\\\0]/.test(file)) {
      throw new Error("Artifact must be a flat filename within --directory");
    }
    const extension = extname(file).toLowerCase();
    if (!(os === "macos" && arch === "universal" && extension === ".pkg")
      && !(os === "linux" && ["arm64", "x64"].includes(arch) && [".deb", ".rpm"].includes(extension))) {
      throw new Error("Artifacts must be macos:universal PKG or linux:arm64/x64 DEB/RPM");
    }
    if (seen.has(file)) throw new Error(`Duplicate artifact filename: ${file}`);
    seen.add(file);
    const info = lstatSync(join(directory, file));
    if (!info.isFile() || info.size === 0) throw new Error(`Artifact must be a nonempty regular file: ${file}`);
    return { os, arch, file };
  });
  const manifest = {
    product: values.product,
    version: values.version,
    published_at: values["published-at"],
    ...(values["notes-url"] !== undefined ? { notes_url: values["notes-url"] } : {}),
    artifacts,
  };
  const path = join(directory, `${values.product}.release.json`);
  writeFileSync(path, `${JSON.stringify(manifest, null, 2)}\n`, { flag: "wx" });
  return path;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    console.log(prepareReleaseManifest(process.argv.slice(2)));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
