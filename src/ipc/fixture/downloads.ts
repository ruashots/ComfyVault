/**
 * The development engine's downloader: the services it pretends to ask, the
 * tokens it pretends to keep, and the queue it runs one transfer at a time.
 *
 * It answers in the shapes and words the real engine uses. The remote side is
 * a small catalog of real models with their real sizes and hashes, so the
 * screens read the same numbers the person would see.
 */

import type {
  LinkFolder,
  LinkRoot,
  AddressPlan,
  AddressReading,
  AddressRefusal,
  Download,
  DownloadHost,
  HfPage,
  Install,
  LinkRecord,
  TokenService,
  TokenStatus,
  VaultError,
} from "~/ipc/contract";
import type { Content, World } from "~/ipc/fixture/world";

const GB = 1024 ** 3;
const MB = 1024 ** 2;

/** Room the engine keeps free on the vault drive after a download. */
export const SPACE_MARGIN_BYTES = 5_000_000_000;

/** The categories ComfyUI knows itself, in its own order, for the folder menu. */
export const CATEGORIES = [
  "checkpoints",
  "loras",
  "vae",
  "text_encoders",
  "diffusion_models",
  "clip_vision",
  "style_models",
  "embeddings",
  "vae_approx",
  "controlnet",
  "gligen",
  "upscale_models",
  "latent_upscale_models",
  "hypernetworks",
  "photomaker",
  "classifiers",
  "model_patches",
  "audio_encoders",
];

/** The folders ComfyUI searches for a category, under `models`, in its order. */
const SEARCHED: Record<string, string[]> = {
  text_encoders: ["text_encoders", "clip"],
  diffusion_models: ["unet", "diffusion_models"],
  controlnet: ["controlnet", "t2i_adapter"],
};

/** Civitai's model type, and the folder it goes in. */
const CIVITAI_FOLDER: Record<string, string> = {
  Checkpoint: "checkpoints",
  LORA: "loras",
  LoCon: "loras",
  DoRA: "loras",
  TextualInversion: "embeddings",
  VAE: "vae",
  Controlnet: "controlnet",
  Upscaler: "upscale_models",
};

// ── reading an address ─────────────────────────────────────────────────────

export type ParsedAddress =
  | { host: "huggingface"; owner: string; repo: string; revision: string; path: string }
  | { host: "civitai"; modelId: number | null; versionId: number | null };

/** The engine's address rules, as `download/address.rs` has them. */
export function parseAddress(text: string): ParsedAddress | "bad" | "hfRepoNotFile" {
  const trimmed = text.trim();
  const rest = trimmed.replace(/^https?:\/\//, "");
  const cut = rest.search(/[?#]/);
  const hostAndPath = cut < 0 ? rest : rest.slice(0, cut);
  const query = cut < 0 ? "" : (rest.slice(cut + 1).split("#")[0] ?? "");
  const slash = hostAndPath.indexOf("/");
  const host = (slash < 0 ? hostAndPath : hostAndPath.slice(0, slash)).toLowerCase();
  const segments = (slash < 0 ? "" : hostAndPath.slice(slash + 1)).split("/").filter(Boolean);

  if (host === "huggingface.co" || host === "www.huggingface.co") {
    const [owner, repo, kind, revision, ...parts] = segments;
    if (!owner || owner === "datasets" || owner === "spaces") return "bad";
    if (!repo || !hfName(owner) || !hfName(repo)) return "bad";
    if (kind === undefined || kind === "tree" || kind === "commits" || kind === "discussions") {
      return "hfRepoNotFile";
    }
    if (kind !== "blob" && kind !== "resolve") return "bad";
    if (revision === undefined || parts.length === 0) return "hfRepoNotFile";
    const decoded = [revision, ...parts].map(decode);
    if (decoded.some((p) => p === "" || p === "." || p === ".." || /[/\\\0]/.test(p))) return "bad";
    return { host: "huggingface", owner, repo, revision: decoded[0]!, path: decoded.slice(1).join("/") };
  }
  if (host === "civitai.com" || host === "www.civitai.com") {
    if (segments[0] === "models" && segments[1] !== undefined) {
      const modelId = number(segments[1]);
      if (modelId === null) return "bad";
      const v = query.split("&").map((p) => p.split("=")).find(([k]) => k === "modelVersionId");
      const versionId = v ? number(v[1] ?? "") : null;
      if (v && versionId === null) return "bad";
      return { host: "civitai", modelId, versionId };
    }
    if (segments.length === 4 && segments[0] === "api" && segments[1] === "download" && segments[2] === "models") {
      const versionId = number(segments[3]!);
      return versionId === null ? "bad" : { host: "civitai", modelId: null, versionId };
    }
    return "bad";
  }
  return "bad";
}

const hfName = (s: string) => s.length <= 96 && s !== "." && s !== ".." && /^[A-Za-z0-9._-]+$/.test(s);
const number = (s: string) => (/^[0-9]{1,15}$/.test(s) && Number(s) > 0 ? Number(s) : null);
const decode = (s: string) => {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
};

// ── the services, as the development engine knows them ─────────────────────

interface RemoteFile {
  id: number;
  name: string;
  sizeBytes: number;
  detail: string;
  /** Null for a small Hugging Face file stored without LFS. */
  sha256: string | null;
}

interface CivitaiModel {
  id: number;
  name: string;
  type: string;
  /** Needs a signed-in account to download. */
  needsLogin: boolean;
  versions: { id: number; name: string; files: RemoteFile[] }[];
}

interface HfFile {
  owner: string;
  repo: string;
  path: string;
  file: RemoteFile;
  gated: boolean;
}

const sha = (seed: string) => {
  let out = "";
  let h = 0x811c9dc5;
  for (let round = 0; out.length < 64; round++) {
    for (const ch of `${seed}|${round}`) {
      h ^= ch.charCodeAt(0);
      h = Math.imul(h, 0x01000193) >>> 0;
    }
    out += h.toString(16).padStart(8, "0");
  }
  return out.slice(0, 64).toUpperCase();
};

const CIVITAI: CivitaiModel[] = [
  {
    id: 4384,
    name: "DreamShaper",
    type: "Checkpoint",
    needsLogin: false,
    versions: [
      {
        id: 128713,
        name: "8",
        files: [
          {
            id: 93152,
            name: "dreamshaper_8.safetensors",
            sizeBytes: 2082642 * 1024,
            detail: "pruned fp16",
            sha256: "879DB523C30D3B9017143D56705015E15A2CB5628762C11D086FED9538ABD7FD",
          },
        ],
      },
      {
        id: 252914,
        name: "8 LCM",
        files: [
          { id: 190998, name: "dreamshaperLCM_8.safetensors", sizeBytes: 2082642 * 1024, detail: "pruned fp16", sha256: sha("dreamshaper-lcm") },
        ],
      },
      {
        id: 131004,
        name: "8-inpainting",
        files: [
          { id: 94844, name: "dreamshaper_8Inpainting.safetensors", sizeBytes: 2082658 * 1024, detail: "pruned fp16", sha256: sha("dreamshaper-inpaint") },
        ],
      },
      {
        id: 109123,
        name: "7",
        files: [
          { id: 82160, name: "dreamshaper_7.safetensors", sizeBytes: 2082642 * 1024, detail: "pruned fp16", sha256: sha("dreamshaper-7") },
          { id: 82161, name: "dreamshaper_7-full.safetensors", sizeBytes: 5.6 * GB, detail: "full fp32", sha256: sha("dreamshaper-7-full") },
        ],
      },
    ],
  },
  {
    id: 123456,
    name: "Studio Portrait XL",
    type: "Checkpoint",
    needsLogin: true,
    versions: [
      {
        id: 654321,
        name: "2",
        files: [
          { id: 700001, name: "studioPortraitXL_v2.safetensors", sizeBytes: 6.5 * GB, detail: "pruned fp16", sha256: sha("studio-portrait") },
        ],
      },
    ],
  },
  {
    id: 58390,
    name: "Detail Tweaker LoRA",
    type: "LORA",
    needsLogin: false,
    versions: [
      {
        id: 62833,
        name: "1.0",
        files: [
          { id: 60001, name: "add_detail.safetensors", sizeBytes: 37 * MB, detail: "fp16", sha256: sha("add-detail") },
        ],
      },
    ],
  },
];

const HF: HfFile[] = [
  {
    owner: "Comfy-Org",
    repo: "flux1-dev",
    path: "flux1-dev-fp8.safetensors",
    gated: false,
    file: {
      id: 0,
      name: "flux1-dev-fp8.safetensors",
      sizeBytes: 17246524772,
      detail: "",
      sha256: "8E91B68084B53A7FC44ED2A3756D821E355AC1A7B6FE29BE760C1DB532F3D88A",
    },
  },
  {
    owner: "black-forest-labs",
    repo: "FLUX.1-dev",
    path: "flux1-dev.safetensors",
    gated: true,
    file: { id: 0, name: "flux1-dev.safetensors", sizeBytes: 23802932552, detail: "", sha256: sha("bfl-flux1-dev") },
  },
  {
    owner: "comfyanonymous",
    repo: "flux_text_encoders",
    path: "t5xxl_fp16.safetensors",
    gated: false,
    // Its hash is the sample world's own t5xxl_fp16, so it can already be in the vault.
    file: { id: 0, name: "t5xxl_fp16.safetensors", sizeBytes: 0, detail: "", sha256: null },
  },
  {
    owner: "Comfy-Org",
    repo: "Wan_2.2_ComfyUI_Repackaged",
    path: "split_files/vae/wan2.2_vae.safetensors",
    gated: false,
    file: { id: 0, name: "wan2.2_vae.safetensors", sizeBytes: 1403 * MB, detail: "", sha256: sha("wan22-vae-remote") },
  },
  {
    owner: "XLabs-AI",
    repo: "flux-RealismLora",
    path: "lora.safetensors",
    gated: false,
    // Small enough to be stored without LFS, so no hash before the download.
    file: { id: 0, name: "lora.safetensors", sizeBytes: 22 * MB, detail: "", sha256: null },
  },
];

const HF_GATED_MESSAGE = (owner: string, repo: string) =>
  `Access to model ${owner}/${repo} is restricted. You must have access to it and be authenticated to access it. Please log in.`;
const HF_NO_ACCESS_MESSAGE = (owner: string, repo: string) =>
  `Access to model ${owner}/${repo} is restricted and you are not in the authorized list. Visit https://huggingface.co/${owner}/${repo} to ask for access.`;
const HF_BAD_TOKEN = "Invalid username or password.";
const CIVITAI_LOGIN = "The creator of this asset requires you to be logged in to download it";
const CIVITAI_BAD_TOKEN = "Invalid API key";

function refuse(
  kind: AddressRefusal["kind"],
  host: DownloadHost | null,
  serviceMessage: string | null,
  page: HfPage | null = null,
  title: string | null = null,
  subtitle: string | null = null,
): AddressRefusal {
  return { kind, host, title, subtitle, serviceMessage, page };
}

function error(code: VaultError["code"], message: string, detail?: string): VaultError {
  return detail === undefined ? { code, message } : { code, message, detail };
}

// ── the desk ────────────────────────────────────────────────────────────────

interface Token {
  value: string;
  ok: boolean;
  account: string | null;
  message: string | null;
}

interface Job extends Download {
  /** The folder each install's link goes in. */
  dirs: Record<string, string>;
  /** What the transfer reads again on a continue. Not part of the record. */
  address: string;
  versionId: number | null;
  fileId: number | null;
  /** The expected hash, known or not before the transfer. */
  expected: string | null;
}

export class DownloadDesk {
  private tokens: Record<TokenService, Token | null> = { huggingface: null, civitai: null };
  /** Hugging Face models whose terms the saved account accepted. */
  private accepted = new Set<string>();
  private jobs: Job[] = [];
  private timer: ReturnType<typeof setInterval> | null = null;
  private corruptNext = false;
  /** How long the sites take to answer a read, in the browser build. */
  private readDelayMs = 0;
  /** The browser build's clock is paused, so a state stays on screen. */
  private held = false;
  private seq = 0;
  /** The installs ticked at the last download, kept by the engine for the person. */
  private downloadInstallIds: string[] | null = null;

  constructor(
    private readonly world: () => World,
    private readonly manual: boolean,
    private readonly tickMs: () => number,
    private readonly opened: string[],
    private readonly send: (r: Download) => void,
  ) {}

  // ── tokens ─────────────────────────────────────────────────────────────────

  async setToken(service: TokenService, token: string): Promise<{ ok: true; account: string | null }> {
    const value = token.trim();
    if (!value) throw error("invalidArgument", "Paste a token first.");
    // The development engine accepts a token unless it says "bad".
    if (/bad/i.test(value)) {
      // The engine's own sentence, and the site's words as the detail.
      throw error(
        "conflict",
        `${service === "huggingface" ? "Hugging Face" : "Civitai"} did not accept this token, so it was not saved.`,
        service === "huggingface" ? HF_BAD_TOKEN : CIVITAI_BAD_TOKEN,
      );
    }
    const account = service === "huggingface" ? "example-user" : null;
    this.tokens[service] = { value, ok: true, account, message: null };
    return { ok: true, account };
  }

  async getTokenStatus(service: TokenService): Promise<TokenStatus> {
    const t = this.tokens[service];
    if (!t) return { saved: false, ok: null, account: null, message: null };
    return { saved: true, ok: t.ok, account: t.ok ? t.account : null, message: t.message };
  }

  async removeToken(service: TokenService): Promise<{ removed: true }> {
    this.tokens[service] = null;
    return { removed: true };
  }

  /** The service stops accepting the saved token, as when it is deleted there. */
  devRevokeToken(service: TokenService): void {
    const t = this.tokens[service];
    if (t) {
      this.tokens[service] = {
        ...t,
        ok: false,
        account: null,
        message: service === "huggingface" ? HF_BAD_TOKEN : CIVITAI_BAD_TOKEN,
      };
    }
  }

  /** The saved Hugging Face account accepts a gated model's terms. */
  devAcceptTerms(owner: string, repo: string): void {
    this.accepted.add(`${owner}/${repo}`.toLowerCase());
  }

  // ── reading ────────────────────────────────────────────────────────────────

  async readModelAddress(args: {
    address: string;
    versionId?: number;
    fileId?: number;
    category?: string;
  }): Promise<AddressReading> {
    if (this.readDelayMs > 0) await new Promise((r) => setTimeout(r, this.readDelayMs));
    const refused = (refusal: AddressRefusal): AddressReading => ({ plan: null, refusal });
    const parsed = parseAddress(args.address);
    if (parsed === "bad") return refused(refuse("badAddress", null, null));
    if (parsed === "hfRepoNotFile") {
      return refused(refuse("hfRepoNotFile", "huggingface", null));
    }

    if (parsed.host === "huggingface") {
      const name = parsed.path.split("/").pop()!;
      const subtitle = `${parsed.owner}/${parsed.repo}`;
      const page = { owner: parsed.owner, repo: parsed.repo };
      const found = HF.find(
        (f) =>
          f.owner.toLowerCase() === parsed.owner.toLowerCase() &&
          f.repo.toLowerCase() === parsed.repo.toLowerCase() &&
          f.path === parsed.path,
      );
      const hf = (kind: AddressRefusal["kind"], words: string) =>
        refused(refuse(kind, "huggingface", words, page, name, subtitle));
      if (!found) return hf("notFound", "Entry not found");
      const token = this.tokens.huggingface;
      if (found.gated) {
        if (!token) return hf("tokenMissing", HF_GATED_MESSAGE(found.owner, found.repo));
        if (!token.ok) return hf("tokenRejected", token.message ?? HF_BAD_TOKEN);
        if (!this.accepted.has(`${found.owner}/${found.repo}`.toLowerCase())) {
          return hf("noAccess", HF_NO_ACCESS_MESSAGE(found.owner, found.repo));
        }
      }
      const file = this.hfFile(found);
      // Hugging Face says nothing about kind: only a folder in the path hints.
      const hinted = parsed.path.split("/").slice(0, -1).find((p) => CATEGORIES.includes(p)) ?? null;
      return {
        plan: this.plan(
          {
            host: "huggingface",
            title: name,
            subtitle,
            versions: [],
            versionId: null,
            files: [],
            fileId: null,
            page,
            modelId: null,
          },
          file,
          hinted,
          hinted ? `its path in the repository is in ${hinted}` : null,
          args.category,
        ),
        refusal: null,
      };
    }

    const model = CIVITAI.find(
      (m) =>
        (parsed.modelId !== null && m.id === parsed.modelId) ||
        (parsed.modelId === null && m.versions.some((v) => v.id === parsed.versionId)),
    );
    if (!model) {
      return refused(refuse("notFound", "civitai", "Model not found"));
    }
    const versionId = args.versionId ?? parsed.versionId ?? model.versions[0]!.id;
    const version = model.versions.find((v) => v.id === versionId) ?? model.versions[0]!;
    const file = version.files.find((f) => f.id === args.fileId) ?? version.files[0]!;
    if (model.needsLogin) {
      const token = this.tokens.civitai;
      if (!token) {
        return refused(refuse("tokenMissing", "civitai", CIVITAI_LOGIN, null, model.name, `https://civitai.com/models/${model.id}`));
      }
      if (!token.ok) {
        return refused(
          refuse(
            "tokenRejected",
            "civitai",
            token.message ?? CIVITAI_BAD_TOKEN,
            null,
            model.name,
            `https://civitai.com/models/${model.id}`,
          ),
        );
      }
    }
    const folder = CIVITAI_FOLDER[model.type] ?? null;
    return {
      plan: this.plan(
        {
          host: "civitai",
          title: model.name,
          // The model's own page, rebuilt: never the pasted text, which can hold a key.
          subtitle: `https://civitai.com/models/${model.id}`,
          versions: model.versions.map((v) => ({ id: v.id, name: v.name })),
          versionId: version.id,
          files: version.files.map((f) => ({
            id: f.id,
            name: f.name,
            sizeBytes: f.sizeBytes,
            detail: f.detail,
          })),
          fileId: file.id,
          page: null,
          modelId: model.id,
        },
        file,
        folder,
        folder ? `Civitai calls it a ${model.type}` : null,
        args.category,
      ),
      refusal: null,
    };
  }

  /** A Hugging Face file, with the sample world's own hash where it has one. */
  private hfFile(found: HfFile): RemoteFile {
    if (found.file.name !== "t5xxl_fp16.safetensors") return found.file;
    const content = this.world().contents.find((c) => c.filename === "t5xxl_fp16.safetensors");
    return {
      ...found.file,
      sizeBytes: content?.bytes ?? 9.1 * GB,
      sha256: content?.sha256 ?? sha("t5xxl"),
    };
  }

  /** The plan: what the site said, and what depends on the folder and the disk. */
  private plan(
    site: Pick<
      AddressPlan,
      "host" | "title" | "subtitle" | "versions" | "versionId" | "files" | "fileId" | "page" | "modelId"
    >,
    file: RemoteFile,
    suggested: string | null,
    because: string | null,
    asked: string | undefined,
  ): AddressPlan {
    const world = this.world();
    const held = file.sha256 ? world.vault.get(file.sha256) : undefined;
    const heldContent = held ? world.contents.find((c) => c.sha256 === held.sha256) : undefined;
    // A file the vault holds already has its folder: the links go there.
    const category = heldContent?.category ?? asked ?? suggested;
    const alreadyInVault = held
      ? { vaultRelPath: `${heldContent?.category ?? ""}\\${held.canonicalName}` }
      : null;

    let vaultName = file.name;
    if (category && !held) {
      const taken = [...world.vault.values()].some((v) => {
        const c = world.contents.find((x) => x.sha256 === v.sha256);
        return c?.category === category && v.canonicalName === file.name && v.sha256 !== file.sha256;
      });
      if (taken && file.sha256) vaultName = tagged(file.name, file.sha256);
    }
    const categories = [
      ...new Set([...CATEGORIES, ...world.contents.map((c) => c.category)]),
    ].sort();
    const last = this.downloadInstallIds;
    return {
      ...site,
      fileName: file.name,
      sizeBytes: file.sizeBytes,
      sha256: file.sha256,
      categories,
      category,
      suggestedCategory: suggested,
      suggestedBecause: because,
      alreadyInVault,
      vaultRelPath: alreadyInVault ? alreadyInVault.vaultRelPath : category ? `${category}\\${vaultName}` : null,
      vaultNameTaken: vaultName !== file.name,
      installs: world.installs.map((install) => {
        const state = this.stateIn(install, category, file);
        const defaultDir = category ? this.defaultDir(install, category) : null;
        return {
          installId: install.id,
          linkPath: defaultDir ? `${defaultDir}\\${file.name}` : null,
          state,
          ticked: state === "free" && (last === null || last.includes(install.id)),
          roots: category ? this.rootsFor(install, category) : [],
          defaultDir,
        };
      }),
      vaultFreeBytes: world.driveReadable ? world.freeBytes : null,
      spaceNeededBytes: alreadyInVault ? 0 : file.sizeBytes + SPACE_MARGIN_BYTES,
    };
  }

  // ── the folder a new link goes in ─────────────────────────────────────────

  private linkDirs = new Map<string, string>();

  /** Every folder ComfyUI reads for a category in an install, in its order. */
  rootsFor(install: Install, category: string): LinkRoot[] {
    const roots: LinkRoot[] = (SEARCHED[category] ?? [category]).map((d) => ({
      path: `${install.modelsDir}\\${d}`,
      origin: "modelsDir" as const,
    }));
    for (const e of install.extraPaths.filter((x) => x.category === category)) {
      // A YAML folder that holds the whole install is never offered.
      if (install.root.toLowerCase().startsWith(e.path.toLowerCase())) continue;
      if (!roots.some((r) => r.path.toLowerCase() === e.path.toLowerCase())) {
        roots.push({ path: e.path, origin: "extraPath" });
      }
    }
    return roots;
  }

  /** The folder remembered for this install and category, or ComfyUI's own. */
  defaultDir(install: Install, category: string): string {
    return (
      this.linkDirs.get(`${install.id}|${category}`) ?? `${install.modelsDir}\\${category}`
    );
  }

  remember(installId: string, category: string, dir: string): void {
    this.linkDirs.set(`${installId}|${category}`, dir);
  }

  /** Refuse a folder outside every folder ComfyUI reads for the category. */
  checkInside(install: Install, category: string, dir: string): void {
    const d = dir.toLowerCase().replace(/\\+$/, "");
    const ok = this.rootsFor(install, category).some((r) => {
      const root = r.path.toLowerCase();
      return d === root || d.startsWith(`${root}\\`);
    });
    if (!ok) {
      throw {
        ...error(
          "pathOutsideBoundary",
          "That folder is not one ComfyUI reads for this kind of model, so a link there would not show. Choose a folder in the list.",
        ),
        path: dir,
      } satisfies VaultError;
    }
  }

  /** The roots, or the subfolders of one folder, as the disk has them. */
  async listLinkFolders(args: {
    installId: string;
    category: string;
    dir?: string;
  }): Promise<LinkFolder[]> {
    const world = this.world();
    const install = world.installs.find((i) => i.id === args.installId);
    if (!install) throw error("notFound", "That install is not registered any more.");
    const roots = this.rootsFor(install, args.category);
    // The sample disk is the folders its files sit in.
    const folders = new Set<string>();
    for (const c of world.contents) {
      for (const copy of c.copies) {
        const at = copy.absPath.lastIndexOf("\\");
        let dir = copy.absPath.slice(0, at);
        while (dir.includes("\\")) {
          folders.add(dir.toLowerCase());
          dir = dir.slice(0, dir.lastIndexOf("\\"));
        }
      }
    }
    for (const dir of this.linkDirs.values()) folders.add(dir.toLowerCase());
    const below = (path: string) =>
      [...folders].filter((f) => f.startsWith(`${path.toLowerCase()}\\`) && !f.slice(path.length + 1).includes("\\"));
    const originOf = (path: string) =>
      roots.find((r) => path.toLowerCase().startsWith(r.path.toLowerCase()))?.origin ?? "modelsDir";
    const entry = (path: string): LinkFolder => ({
      path,
      name: path.slice(path.lastIndexOf("\\") + 1),
      origin: originOf(path),
      exists: folders.has(path.toLowerCase()),
      hasSubfolders: below(path).length > 0,
    });
    if (args.dir === undefined) return roots.map((r) => entry(r.path));
    this.checkInside(install, args.category, args.dir);
    return below(args.dir)
      .map((f) => `${args.dir}${f.slice(args.dir!.length)}`)
      .map((p) => entry(p))
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  private stateIn(
    install: Install,
    category: string | null,
    file: RemoteFile,
  ): "free" | "hasLink" | "nameTaken" {
    const world = this.world();
    if (file.sha256 && world.links.some((l) => l.installId === install.id && l.sha256 === file.sha256)) {
      return "hasLink";
    }
    if (!category) return "free";
    const folders = (SEARCHED[category] ?? [category]).map((d) => `${install.modelsDir}\\${d}\\`.toLowerCase());
    for (const e of install.extraPaths.filter((x) => x.category === category)) {
      folders.push(`${e.path}\\`.toLowerCase());
    }
    const clash = world.contents.some(
      (c) =>
        c.sha256 !== file.sha256 &&
        c.copies.some(
          (copy) =>
            copy.installId === install.id &&
            copy.name.toLowerCase() === file.name.toLowerCase() &&
            folders.some((f) => copy.absPath.toLowerCase() === `${f}${file.name.toLowerCase()}`),
        ),
    );
    return clash ? "nameTaken" : "free";
  }

  /** A different file with this name, in an install's folder for a category. */
  devPlaceFile(installId: string, category: string, name: string): void {
    const world = this.world();
    const install = world.installs.find((i) => i.id === installId);
    if (!install) throw new Error("devPlaceFile: no such install");
    const folder = `models\\${category}\\`;
    const absPath = `${install.root}\\${folder}${name}`;
    world.contents.push({
      sha256: sha(`placed|${absPath}`),
      filename: name,
      category,
      bytes: 3 * GB,
      workflowHits: 0,
      civitai: null,
      copies: [
        {
          installId,
          folder,
          name,
          absPath,
          relPath: `${folder}${name}`,
          volume: absPath.slice(0, 2).toUpperCase(),
          isLink: false,
          blocked: null,
        },
      ],
    });
  }

  async openHuggingFacePage(owner: string, repo: string): Promise<null> {
    if (!/^[A-Za-z0-9._-]+$/.test(owner) || !/^[A-Za-z0-9._-]+$/.test(repo)) {
      throw error("invalidArgument", "That is not the name of a Hugging Face model.");
    }
    this.opened.push(`https://huggingface.co/${owner}/${repo}`);
    return null;
  }

  // ── the queue ──────────────────────────────────────────────────────────────

  private emit(job: Job): void {
    this.send(publicOf(job));
  }

  async listDownloads(): Promise<Download[]> {
    return this.jobs.map(publicOf);
  }

  async startDownload(args: {
    address: string;
    versionId?: number;
    fileId?: number;
    category: string;
    installIds: string[];
    links?: { installId: string; dir: string }[];
  }): Promise<Download> {
    if (!CATEGORIES.includes(args.category)) {
      throw error("invalidArgument", "That folder name cannot be used. Choose one from the list.");
    }
    const reading = await this.readModelAddress(args);
    if (reading.refusal || !reading.plan) {
      const r = reading.refusal!;
      const site = r.host === "civitai" ? "Civitai" : r.host === "huggingface" ? "Hugging Face" : "The site";
      const said = {
        tokenMissing: `${site} needs your token for this model. Add it in Settings, then read the address again.`,
        tokenRejected: `${site} did not accept your token. Paste a new one in Settings.`,
        noAccess: `Your ${site} account has no access to this model yet.`,
        notFound: `${site} has no such file.`,
        badAddress: "That is not the address of one model file.",
        hfRepoNotFile: "That is not the address of one model file.",
      }[r.kind];
      throw error("conflict", said, r.serviceMessage ?? undefined);
    }
    const plan = reading.plan;
    const unknown = (args.links?.map((l) => l.installId) ?? args.installIds).find(
      (id) => !this.world().installs.some((i) => i.id === id),
    );
    if (unknown) throw error("notFound", "One of the chosen installs is not registered any more.");
    const world = this.world();
    const asked = args.links ? args.links.map((l) => l.installId) : args.installIds;
    const installIds = asked.filter((id) =>
      plan.installs.some((i) => i.installId === id && i.state === "free"),
    );
    // Each chosen folder must be one ComfyUI reads for this category.
    const dirs: Record<string, string> = {};
    for (const id of installIds) {
      const install = world.installs.find((i) => i.id === id)!;
      const chosen = args.links?.find((l) => l.installId === id)?.dir;
      const dir = chosen ?? this.defaultDir(install, plan.category ?? args.category);
      this.checkInside(install, plan.category ?? args.category, dir);
      dirs[id] = dir;
    }
    const job: Job = {
      downloadId: `download-${++this.seq}`,
      host: plan.host,
      title: plan.title,
      // Kept without any key the pasted text carried, as the engine keeps it.
      address: args.address.trim().replace(/([?&])token=[^&#]*&?/i, "$1").replace(/[?&]$/, ""),
      versionId: plan.versionId,
      fileId: plan.fileId,
      fileName: plan.fileName,
      bytesTotal: plan.sizeBytes,
      bytesDone: 0,
      bytesPerSecond: null,
      state: "waiting",
      category: plan.category ?? args.category,
      vaultRelPath: plan.vaultRelPath ?? `${args.category}\\${plan.fileName}`,
      sha256: plan.alreadyInVault ? plan.sha256 : null,
      installIds,
      linkedInstallIds: [],
      notLinked: [],
      alreadyInVault: false,
      error: null,
      startedAt: new Date().toISOString(),
      finishedAt: null,
      expected: plan.sha256,
      dirs,
    };
    // The engine remembers the ticks, and each folder, for the next card.
    this.downloadInstallIds = [...asked];
    for (const [id, dir] of Object.entries(dirs)) this.remember(id, job.category, dir);
    if (plan.alreadyInVault) {
      // Nothing to transfer: the links are made at once.
      job.linkedInstallIds = this.link(job, plan.sha256!);
      job.state = "linkedOnly";
      job.alreadyInVault = true;
      job.finishedAt = new Date().toISOString();
      this.jobs.push(job);
      this.emit(job);
      return publicOf(job);
    }
    if (plan.vaultFreeBytes === null) {
      throw error(
        "ioError",
        "ComfyVault could not read how much free space the vault's drive has, so the download did not start. Check that the drive is connected, then try again.",
      );
    }
    if (plan.vaultFreeBytes < plan.spaceNeededBytes) {
      throw error(
        "ioError",
        "There is not enough free space on the vault's drive for this file and the 5 GB kept free.",
        `needs ${plan.spaceNeededBytes} bytes, ${plan.vaultFreeBytes} free`,
      );
    }
    void world;
    this.jobs.push(job);
    this.emit(job);
    this.schedule();
    return publicOf(this.jobs.find((j) => j.downloadId === job.downloadId)!);
  }

  private find(downloadId: string): Job {
    const job = this.jobs.find((j) => j.downloadId === downloadId);
    if (!job) throw error("notFound", "That download is not in the list any more.");
    return job;
  }

  async stopDownload(downloadId: string): Promise<Download> {
    const job = this.find(downloadId);
    if (!["waiting", "running", "checking"].includes(job.state)) {
      throw error("conflict", "That download is not running, so there is nothing to stop.");
    }
    // A running transfer may still be letting go of its connection.
    if (job.state === "running" && this.slowStop) this.stopping.add(job.downloadId);
    job.state = "stopped";
    job.bytesPerSecond = null;
    this.emit(job);
    this.schedule();
    return publicOf(job);
  }

  /** Downloads stopped while their connection is still closing. */
  private stopping = new Set<string>();
  /** The next stop leaves its connection closing until devLetGo. */
  private slowStop = false;

  /** A stopped transfer takes a while to let go of its connection. */
  devSlowStop(on = true): void {
    this.slowStop = on;
  }

  /** The stopped transfers have let go of their connections. */
  devLetGo(): void {
    this.stopping.clear();
  }

  async continueDownload(downloadId: string): Promise<Download> {
    const job = this.find(downloadId);
    if (!["stopped", "failed", "cutOff", "mismatch"].includes(job.state)) {
      throw error("conflict", "Only a stopped, failed or cut-off download can continue.");
    }
    if (this.stopping.has(downloadId)) {
      throw error("conflict", "That download is still stopping. Try again in a moment.");
    }
    // A file that did not match starts again from nothing.
    if (job.state === "mismatch") job.bytesDone = 0;
    job.state = "waiting";
    job.error = null;
    this.emit(job);
    this.schedule();
    return publicOf(job);
  }

  async discardDownload(downloadId: string): Promise<{ removed: true }> {
    const job = this.find(downloadId);
    if (["waiting", "running", "checking"].includes(job.state) || this.stopping.has(downloadId)) {
      throw error("conflict", "Stop that download first, then discard it.");
    }
    this.jobs = this.jobs.filter((j) => j !== job);
    return { removed: true };
  }

  async removeDownload(downloadId: string): Promise<{ removed: true }> {
    const job = this.find(downloadId);
    const noPart = job.state === "waiting" && job.bytesDone === 0;
    if (!noPart && !["done", "linkedOnly", "mismatch"].includes(job.state)) {
      throw error(
        "conflict",
        "That download has a part already downloaded. Discard it to delete the part, or continue it.",
      );
    }
    this.jobs = this.jobs.filter((j) => j !== job);
    this.schedule();
    return { removed: true };
  }

  /** Start the next waiting download when none runs. */
  private schedule(): void {
    if (!this.jobs.some((j) => j.state === "running" || j.state === "checking")) {
      const next = this.jobs.find((j) => j.state === "waiting");
      if (next && !this.world().driveReadable) {
        // A drive that cannot say what it has free is not written to.
        next.state = "failed";
        next.error = {
          kind: "noSpace",
          message:
            "ComfyVault could not read how much free space the vault's drive has, so the download did not start. Check that the drive is connected, then try again.",
          serviceMessage: null,
          detail: null,
        };
        this.emit(next);
      } else if (next) {
        next.state = "running";
        this.emit(next);
      }
    }
    const busy = this.jobs.some((j) => j.state === "running" || j.state === "checking");
    if (busy && !this.timer && !this.manual && !this.held) {
      this.timer = setInterval(() => this.devStep(), this.tickMs());
    }
    if (!busy && this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }

  /** One step of the running download: a sixtieth of the file, then the check. */
  devStep(): void {
    const job = this.jobs.find((j) => j.state === "running" || j.state === "checking");
    if (!job) return this.schedule();
    if (job.state === "checking") {
      this.settle(job);
    } else {
      const chunk = Math.max(1, Math.ceil(job.bytesTotal / 60));
      job.bytesDone = Math.min(job.bytesTotal, job.bytesDone + chunk);
      job.bytesPerSecond = Math.min(chunk, 38 * MB);
      if (job.bytesDone >= job.bytesTotal) {
        job.state = "checking";
        job.bytesPerSecond = null;
      }
      this.emit(job);
    }
    this.schedule();
  }

  /** Run the queue until nothing is running or waiting. */
  devFinishDownloads(limit = 100000): void {
    for (let i = 0; i < limit; i++) {
      if (!this.jobs.some((j) => ["running", "checking", "waiting"].includes(j.state))) return;
      this.devStep();
    }
  }

  /** The bytes checked, then the vault record and the links. */
  private settle(job: Job): void {
    const world = this.world();
    if (this.corruptNext) {
      this.corruptNext = false;
      job.state = "mismatch";
      job.bytesDone = 0;
      job.finishedAt = new Date().toISOString();
      this.emit(job);
      return;
    }
    const hash = job.expected ?? sha(`computed|${job.address}|${job.fileName}`);
    job.sha256 = hash;
    job.finishedAt = new Date().toISOString();
    if (world.vault.has(hash)) {
      // Found after the transfer: the new file goes, the links are made.
      job.linkedInstallIds = this.link(job, hash);
      job.state = "linkedOnly";
      job.alreadyInVault = true;
      this.emit(job);
      return;
    }
    const name = job.vaultRelPath.split("\\").pop()!;
    const content: Content = {
      sha256: hash,
      filename: name,
      category: job.category,
      bytes: job.bytesTotal,
      workflowHits: 0,
      civitai: null,
      copies: [],
    };
    world.contents.push(content);
    world.vault.set(hash, { sha256: hash, canonicalName: name, aliases: [], addedAt: job.finishedAt });
    world.freeBytes -= job.bytesTotal;
    job.linkedInstallIds = this.link(job, hash);
    job.state = "done";
    this.emit(job);
  }

  /** A link in each asked install, as create_link makes it. */
  private link(job: Job, hash: string): string[] {
    const world = this.world();
    const content = world.contents.find((c) => c.sha256 === hash);
    const entry = world.vault.get(hash)!;
    const done: string[] = [];
    for (const installId of job.installIds) {
      const install = world.installs.find((i) => i.id === installId);
      if (!install) continue;
      const dir = job.dirs[installId] ?? this.defaultDir(install, job.category);
      const absPath = `${dir}\\${job.fileName}`;
      const folder = relOf(install, dir);
      if (world.links.some((l) => l.absPath === absPath)) continue;
      const link: LinkRecord = {
        id: `link-${world.links.length + 1}-${job.downloadId}`,
        installId,
        absPath,
        relPath: `${folder}${job.fileName}`,
        linkName: job.fileName,
        sha256: hash,
        vaultRelPath: `${content?.category ?? job.category}\\${entry.canonicalName}`,
        createdAt: new Date().toISOString(),
        createdBy: "download",
        applyId: null,
      };
      world.links.push(link);
      content?.copies.push({
        installId,
        folder,
        name: job.fileName,
        absPath,
        relPath: `${folder}${job.fileName}`,
        volume: absPath.slice(0, 2).toUpperCase(),
        isLink: true,
        blocked: null,
      });
      done.push(installId);
    }
    return done;
  }

  // ── dev switches for the states the screens must show ─────────────────────

  /** The connection drops under the running download. */
  devDropConnection(): void {
    this.failRunning({
      kind: "connection",
      message: `The connection to ${this.runningSite()} dropped. The part already downloaded is kept.`,
      serviceMessage: null,
      detail: "connection reset by peer",
    });
  }

  /** The service answers 401 or 403 in the middle, in its own words. */
  devRefuseMidway(message: string): void {
    this.failRunning({
      kind: "refused",
      message: `${this.runningSite()} refused the download part way.`,
      serviceMessage: message,
      detail: null,
    });
  }

  private runningSite(): string {
    const job = this.jobs.find((j) => j.state === "running");
    return job?.host === "civitai" ? "Civitai" : "Hugging Face";
  }

  private failRunning(err: NonNullable<Download["error"]>): void {
    const job = this.jobs.find((j) => j.state === "running");
    if (!job) throw new Error("no download is running");
    job.state = "failed";
    job.error = err;
    job.bytesPerSecond = null;
    this.emit(job);
    this.schedule();
  }

  /** Pause or restart the clock that moves a download along. */
  devHold(on: boolean): void {
    this.held = on;
    if (on && this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
    if (!on) this.schedule();
  }

  /** The sites take this long to answer, so the reading card can be seen. */
  devSetReadDelay(ms: number): void {
    this.readDelayMs = ms;
  }

  /** The vault drive has this much free. */
  devSetFreeBytes(bytes: number): void {
    this.world().freeBytes = bytes;
  }

  /** The next download that reaches its check does not match. */
  devCorruptNext(): void {
    this.corruptNext = true;
  }

  /**
   * ComfyVault closes under the running download and opens again: the record
   * comes back from the list as cut off, with its part kept.
   */
  devCutOff(): void {
    const under = this.jobs.filter((j) => ["waiting", "running", "checking"].includes(j.state));
    if (under.length === 0) throw new Error("no download is under way");
    // After a restart nothing starts by itself, and the finished ones are gone.
    this.jobs = this.jobs.filter((j) => !["done", "linkedOnly", "mismatch"].includes(j.state));
    for (const job of under) {
      job.state = "cutOff";
      job.bytesPerSecond = null;
      // Stands in for the window reading the list again after the restart.
      this.emit(job);
    }
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }

  dispose(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }
}

/** "model.safetensors" + a hash -> "model__4898C16F.safetensors" */
function tagged(name: string, hash: string): string {
  const dot = name.lastIndexOf(".");
  const tag = `__${hash.slice(0, 8).toUpperCase()}`;
  return dot <= 0 ? name + tag : name.slice(0, dot) + tag + name.slice(dot);
}

/** A folder inside an install, from the install folder on, with a trailing \\. */
function relOf(install: Install, dir: string): string {
  const root = `${install.root}\\`;
  return (dir.toLowerCase().startsWith(root.toLowerCase()) ? dir.slice(root.length) : dir) + "\\";
}

function publicOf(job: Job): Download {
  const { expected: _e, address: _a, versionId: _v, fileId: _f, dirs: _d, ...record } = job;
  return { ...record, installIds: [...record.installIds], linkedInstallIds: [...record.linkedInstallIds] };
}

