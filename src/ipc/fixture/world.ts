/**
 * The development world.
 *
 * It holds the same shape of facts a real machine holds: installs, files on
 * disk, a vault, and a couple of things that are in the way. Every contract
 * object the development engine returns is derived from it, using the same
 * rules docs/IPC-CONTRACT.md says the engine uses, so a screen cannot pass
 * against the fixture and fail against the engine.
 *
 * The dataset is copied from design/mock/comfyvault.html, so the figures on
 * screen can be compared against the blessed mock.
 *
 * Nothing here reads a disk. It never sees a real ComfyUI install.
 */

import type {
  BlockReason,
  BlockedRow,
  ConsolidationPlan,
  ContentRow,
  Install,
  InstallScanTotals,
  Link,
  ModelMetadata,
  NameGroup,
  PlanGroup,
  PlanLink,
  PlanSource,
  ScanEntry,
  ScanResult,
  ScanTotals,
  VaultFile,
} from "~/ipc/contract";

const MB = 1024 * 1024;

export const VAULT_ROOT = "C:\\ComfyVault";
export const VAULT_VOLUME = "C:";
export const VAULT_TOTAL_BYTES = 1908408 * MB;
export const VAULT_FREE_BYTES = 139264 * MB;

/** [filename, category, megabytes, workflowHits, copies, civitai?] */
type Row = [
  string,
  string,
  number,
  number,
  string[],
  [string, string, string, string, string]?,
];

/** A copy: "installId:folder" + optional " >nameUsedThere" + optional " !locked|denied|drive" */
const ROWS: Row[] = [
  ["wan2.1_i2v_720p_14B_fp8_scaled.safetensors","diffusion_models",16793,7,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\wan\\"]],
  ["wan2.1_i2v_480p_14B_fp8_scaled.safetensors","diffusion_models",16793,4,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\wan\\"]],
  ["wan2.1_t2v_14B_fp8_scaled.safetensors","diffusion_models",16793,3,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\wan\\"]],
  ["wan2.1_i2v_480p_14B_bf16.safetensors","diffusion_models",33587,0,["prod:models\\diffusion_models\\bf16\\","norm:models\\diffusion_models\\wan\\"]],
  ["wan2.1_t2v_1.3B_fp16.safetensors","diffusion_models",5836,2,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\"]],
  ["wan2.2_i2v_high_noise_14B_fp8_scaled.safetensors","diffusion_models",14234,9,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\wan22\\"]],
  ["wan2.2_i2v_low_noise_14B_fp8_scaled.safetensors","diffusion_models",14234,9,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\wan22\\"]],
  ["wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors","diffusion_models",14234,3,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\wan22\\"]],
  ["wan2.2_t2v_low_noise_14B_fp8_scaled.safetensors","diffusion_models",14234,3,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\wan22\\"]],
  ["wan2.2_animate_14B_fp8_scaled.safetensors","diffusion_models",14234,4,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\wan22\\"]],
  ["wan2.2_s2v_14B_fp8_scaled.safetensors","diffusion_models",14234,1,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\wan22\\"]],
  ["wan2.2_ti2v_5B_fp16.safetensors","diffusion_models",10342,2,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\wan22\\"]],
  ["ltxv-13b-0.9.7-dev.safetensors","checkpoints",27034,5,["prod:models\\checkpoints\\","norm:D:\\ai-models\\ltx\\ !drive"]],
  ["ltxv-13b-0.9.8-distilled.safetensors","checkpoints",13517,4,["prod:models\\checkpoints\\","norm:D:\\ai-models\\ltx\\ !drive"]],
  ["ltx-video-2b-v0.9.5.safetensors","checkpoints",9421,0,["prod:models\\checkpoints\\","norm:D:\\ai-models\\ltx\\ !drive"]],
  ["flux1-dev.safetensors","diffusion_models",24371,8,["prod:models\\diffusion_models\\ !locked","prod:models\\checkpoints\\flux\\","norm:models\\unet\\"]],
  ["flux1-schnell.safetensors","diffusion_models",24371,1,["prod:models\\diffusion_models\\","norm:models\\unet\\"]],
  ["flux1-fill-dev.safetensors","diffusion_models",24371,2,["prod:models\\diffusion_models\\","norm:models\\unet\\"]],
  ["flux1-krea-dev.safetensors","diffusion_models",24371,3,["prod:models\\diffusion_models\\","norm:models\\unet\\"]],
  ["flux1-kontext-dev.safetensors","diffusion_models",24371,6,["prod:models\\diffusion_models\\","norm:models\\unet\\"]],
  ["flux1-dev-fp8.safetensors","checkpoints",12186,0,["prod:models\\checkpoints\\","norm:models\\checkpoints\\"]],
  ["hunyuan_video_t2v_720p_bf16.safetensors","diffusion_models",26214,2,["prod:models\\diffusion_models\\ !locked","norm:models\\diffusion_models\\hunyuan\\"]],
  ["hunyuan_video_i2v_720p_bf16.safetensors","diffusion_models",26214,1,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\hunyuan\\"]],
  ["hunyuan_video_t2v_720p_fp8_scaled.safetensors","diffusion_models",13107,0,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\hunyuan\\"]],
  ["hunyuan_video_i2v_720p_fp8_scaled.safetensors","diffusion_models",13107,0,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\hunyuan\\"]],
  ["cosmos-1_0-diffusion-7b-text2world.safetensors","diffusion_models",14541,0,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\cosmos\\"]],
  ["cosmos_predict2_2B_t2i.safetensors","diffusion_models",4403,0,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\cosmos\\"]],
  ["mochi_preview_bf16.safetensors","diffusion_models",20889,0,["norm:models\\diffusion_models\\"]],
  ["qwen_image_fp8_e4m3fn.safetensors","diffusion_models",20889,6,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\qwen\\ !denied"]],
  ["qwen_image_edit_2509_fp8.safetensors","diffusion_models",20889,5,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\qwen\\"]],
  ["qwen_image_edit_fp8_e4m3fn.safetensors","diffusion_models",20889,2,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\qwen\\"]],
  ["qwen_image_distill_full_fp8.safetensors","diffusion_models",20889,1,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\qwen\\"]],
  ["hidream_i1_full_fp8.safetensors","diffusion_models",17510,0,["prod:models\\diffusion_models\\"]],
  ["svd_xt_1_1.safetensors","checkpoints",9789,0,["prod:models\\checkpoints\\"]],
  ["sd3.5_large.safetensors","checkpoints",16896,2,["prod:models\\checkpoints\\","norm:models\\checkpoints\\sd35\\"]],
  ["sd3.5_large_turbo.safetensors","checkpoints",16896,0,["prod:models\\checkpoints\\","norm:models\\checkpoints\\sd35\\"]],
  ["sd3.5_medium.safetensors","checkpoints",5222,0,["prod:models\\checkpoints\\","norm:models\\checkpoints\\sd35\\"]],
  ["sd_xl_base_1.0.safetensors","checkpoints",7106,4,["prod:models\\checkpoints\\","prod:models\\checkpoints\\sdxl\\ !locked","norm:models\\checkpoints\\"]],
  ["sd_xl_refiner_1.0.safetensors","checkpoints",6226,1,["prod:models\\checkpoints\\","norm:models\\checkpoints\\"]],
  ["sd_xl_turbo_1.0_fp16.safetensors","checkpoints",7106,0,["prod:models\\checkpoints\\","norm:models\\checkpoints\\"]],
  ["juggernautXL_v9Rundiffusionphoto2.safetensors","checkpoints",7280,3,["prod:models\\checkpoints\\","norm:models\\checkpoints\\sdxl\\"],["Juggernaut XL","V9 + RunDiffusion Photo 2","Checkpoint","SDXL 1.0","RunDiffusion"]],
  ["dreamshaperXL_v21TurboDPMSDE.safetensors","checkpoints",6615,2,["prod:models\\checkpoints\\","norm:models\\checkpoints\\sdxl\\"],["DreamShaper XL","v2.1 Turbo DPM++ SDE","Checkpoint","SDXL 1.0","Lykon"]],
  ["realvisxlV50_v50Bakedvae.safetensors","checkpoints",6615,0,["prod:models\\checkpoints\\","norm:models\\checkpoints\\sdxl\\"],["RealVisXL V5.0","V5.0 (BakedVAE)","Checkpoint","SDXL 1.0","SG_161222"]],
  ["epicrealismXL_v10.safetensors","checkpoints",6615,0,["prod:models\\checkpoints\\","norm:models\\checkpoints\\sdxl\\"],["epiCRealism XL","v10","Checkpoint","SDXL 1.0","epinikion"]],
  ["animagineXL_v31.safetensors","checkpoints",6615,0,["prod:models\\checkpoints\\","norm:models\\checkpoints\\sdxl\\"],["Animagine XL","3.1","Checkpoint","SDXL 1.0","Linaqruf"]],
  ["ponyDiffusionV6XL_v6.safetensors","checkpoints",6615,1,["prod:models\\checkpoints\\","norm:models\\checkpoints\\sdxl\\"],["Pony Diffusion V6 XL","V6 (start with this one)","Checkpoint","SDXL 1.0","PurpleSmartAI"]],
  ["v1-5-pruned-emaonly.safetensors","checkpoints",4068,0,["prod:models\\checkpoints\\sd15\\"]],
  ["realisticVisionV60B1_v51VAE.safetensors","checkpoints",2132,0,["norm:models\\checkpoints\\sd15\\"],["Realistic Vision V6.0 B1","V5.1 (VAE)","Checkpoint","SD 1.5","SG_161222"]],
  ["umt5_xxl_fp8_e4m3fn_scaled.safetensors","text_encoders",6902,12,["prod:models\\text_encoders\\","norm:models\\text_encoders\\umt5\\ >umt5-xxl-enc-fp8_e4m3fn.safetensors"]],
  ["umt5_xxl_fp16.safetensors","text_encoders",11264,0,["norm:models\\text_encoders\\umt5\\"]],
  ["t5xxl_fp16.safetensors","text_encoders",10025,6,["prod:models\\text_encoders\\","norm:models\\clip\\ >t5xxl_fp16_googleT5.safetensors"]],
  ["t5xxl_fp8_e4m3fn.safetensors","text_encoders",5007,3,["prod:models\\text_encoders\\","norm:models\\clip\\"]],
  ["clip_l.safetensors","text_encoders",241,9,["prod:models\\text_encoders\\","norm:models\\clip\\"]],
  ["clip_g.safetensors","text_encoders",1424,2,["prod:models\\text_encoders\\","norm:models\\clip\\"]],
  ["llava_llama3_fp8_scaled.safetensors","text_encoders",9308,2,["prod:models\\text_encoders\\","norm:models\\text_encoders\\"]],
  ["qwen_2.5_vl_7b_fp8_scaled.safetensors","text_encoders",8520,6,["prod:models\\text_encoders\\","norm:models\\text_encoders\\qwen\\"]],
  ["wan_2.1_vae.safetensors","vae",249,13,["prod:models\\vae\\","norm:models\\vae\\wan\\ >Wan2_1_VAE_bf16.safetensors"]],
  ["wan2.2_vae.safetensors","vae",1372,4,["prod:models\\vae\\","norm:models\\vae\\wan\\"]],
  ["ae.safetensors","vae",343,8,["prod:models\\vae\\","norm:models\\vae\\flux\\ >flux_vae.safetensors"]],
  ["hunyuan_video_vae_bf16.safetensors","vae",505,3,["prod:models\\vae\\","norm:models\\vae\\"]],
  ["ltxv-vae-0.9.7.safetensors","vae",1649,5,["prod:models\\vae\\","norm:D:\\ai-models\\ltx\\ !drive"]],
  ["sdxl_vae.safetensors","vae",327,4,["prod:models\\vae\\","norm:models\\vae\\"]],
  ["qwen_image_vae.safetensors","vae",249,6,["prod:models\\vae\\","norm:models\\vae\\qwen\\"]],
  ["vae-ft-mse-840000-ema-pruned.safetensors","vae",327,0,["prod:models\\vae\\","norm:models\\vae\\"]],
  ["Wan21_CausVid_14B_T2V_lora_rank32.safetensors","loras",1352,5,["prod:models\\loras\\awesomeloras\\","prod:models\\loras\\","norm:models\\loras\\newloras\\"]],
  ["Wan2.1_I2V_14B_lightx2v_cfg_step_distill_lora_rank64.safetensors","loras",1270,7,["prod:models\\loras\\awesomeloras\\","norm:models\\loras\\newloras\\"]],
  ["wan2.2_i2v_lightx2v_4step_lora_high_noise.safetensors","loras",625,6,["prod:models\\loras\\","norm:models\\loras\\newloras\\"]],
  ["wan2.2_i2v_lightx2v_4step_lora_low_noise.safetensors","loras",625,6,["prod:models\\loras\\","norm:models\\loras\\newloras\\"]],
  ["flux1-turbo-alpha.safetensors","loras",711,1,["prod:models\\loras\\","norm:models\\loras\\flux\\"]],
  ["ltxv-13b-0.9.7-distilled-lora128.safetensors","loras",1086,2,["prod:models\\loras\\","norm:models\\loras\\"]],
  ["add_detail_xl.safetensors","loras",173,1,["prod:models\\loras\\sdxl\\","norm:models\\loras\\"],["Detail Tweaker XL","v1.0","LORA","SDXL 1.0","Kybalico"]],
  ["detail_tweaker_xl.safetensors","loras",233,0,["prod:models\\loras\\sdxl\\","norm:models\\loras\\"],["Detail Tweaker XL","v2.0","LORA","SDXL 1.0","Kybalico"]],
  ["sdxl_lightning_8step_lora.safetensors","loras",394,0,["prod:models\\loras\\sdxl\\","norm:models\\loras\\"]],
  ["hyper-sd15-8steps-lora.safetensors","loras",136,0,["norm:models\\loras\\"],["Hyper-SD","8 steps LoRA","LORA","SD 1.5","ByteDance"]],
  ["4x-UltraSharp.pth","upscale_models",66,11,["prod:models\\upscale_models\\","prod:models\\upscale_models\\esrgan\\","norm:models\\upscale_models\\ >4x_ultrasharp.pth"]],
  ["RealESRGAN_x4plus.pth","upscale_models",66,2,["prod:models\\upscale_models\\","norm:models\\upscale_models\\"]],
  ["4x_NMKD-Siax_200k.pth","upscale_models",66,1,["prod:models\\upscale_models\\","norm:models\\upscale_models\\"]],
  ["4x_foolhardy_Remacri.pth","upscale_models",66,0,["prod:models\\upscale_models\\"]],
  ["1x_ITF_SkinDiffDetail_Lite_v1.pth","upscale_models",5,0,["prod:models\\upscale_models\\"]],
  ["controlnet-union-sdxl-1.0.safetensors","controlnet",2570,2,["prod:models\\controlnet\\","norm:models\\controlnet\\"]],
  ["flux-controlnet-canny-v3.safetensors","controlnet",3666,0,["prod:models\\controlnet\\","norm:models\\controlnet\\flux\\"]],
  ["control_v11p_sd15_openpose.pth","controlnet",1445,0,["prod:models\\controlnet\\sd15\\"]],
  ["t2i-adapter-depth-midas-sdxl.safetensors","controlnet",158,0,["norm:models\\controlnet\\"]],
  ["clip_vision_h.safetensors","clip_vision",1290,4,["prod:models\\clip_vision\\","norm:models\\clip_vision\\"]],
  ["sigclip_vision_patch14_384.safetensors","clip_vision",828,3,["prod:models\\clip_vision\\","norm:models\\clip_vision\\sigclip\\"]],
  ["CLIP-ViT-H-14-laion2B-s32B-b79K.safetensors","clip_vision",2528,0,["norm:models\\clip_vision\\"]],
  // same file name, different bytes
  ["diffusion_pytorch_model.safetensors","controlnet",2508,1,["prod:models\\controlnet\\union\\"]],
  ["diffusion_pytorch_model.safetensors","controlnet",1409,0,["norm:models\\controlnet\\openpose\\"]],
  ["vae.safetensors","vae",351,2,["prod:models\\vae\\"]],
  ["vae.safetensors","vae",319,1,["norm:models\\vae\\"]],
  ["model.safetensors","clip_vision",1246,0,["prod:models\\clip_vision\\"]],
  ["model.safetensors","clip_vision",812,1,["norm:models\\clip_vision\\sigclip\\"]],
  ["pytorch_lora_weights.safetensors","loras",689,0,["prod:models\\loras\\flux\\"]],
  ["pytorch_lora_weights.safetensors","loras",402,1,["norm:models\\loras\\"]],
  // already in the vault, nothing links to them: ComfyUI-Portable was removed
  ["sd15_inpainting_v1.5.safetensors","checkpoints",4068,0,[]],
  ["deliberate_v2.safetensors","checkpoints",2132,0,[],["Deliberate","v2","Checkpoint","SD 1.5","XpucT"]],
  ["control_v11f1p_sd15_depth.pth","controlnet",1445,0,[]],
];

const BLOCK_FLAGS: Record<string, BlockReason> = {
  locked: "fileLocked",
  denied: "permissionDenied",
};

export interface Copy {
  installId: string;
  /** Folder as written in the install, or an absolute folder from the yaml. */
  folder: string;
  name: string;
  absPath: string;
  relPath: string;
  volume: string;
  /** Set once Apply has put a link here. */
  isLink: boolean;
  blocked: BlockReason | null;
}

export interface Content {
  sha256: string;
  filename: string;
  category: string;
  bytes: number;
  workflowHits: number;
  copies: Copy[];
  metadata: ModelMetadata | null;
}

export interface VaultEntry {
  sha256: string;
  canonicalName: string;
  aliases: string[];
  addedAt: string;
}

export interface World {
  installs: Install[];
  contents: Content[];
  vault: Map<string, VaultEntry>;
  links: Link[];
  freeBytes: number;
  /** ComfyUI is running out of these installs. */
  running: string[];
  symlinksSupported: boolean;
  metadataLookupsEnabled: boolean;
  /** Saved workflow files there are to search. Zero means nothing was searched. */
  workflowsOnDisk: number;
}

/** A stable stand-in for a real SHA-256, so identifiers never move. */
function fakeSha(seed: string): string {
  let out = "";
  let h = 0x811c9dc5;
  for (let round = 0; round < 8; round++) {
    const input = `${seed}|${round}`;
    for (let i = 0; i < input.length; i++) {
      h ^= input.charCodeAt(i);
      h = Math.imul(h, 0x01000193) >>> 0;
    }
    out += h.toString(16).padStart(8, "0");
  }
  return out.toUpperCase();
}

const INSTALL_ROOTS: Record<string, string> = {
  prod: "C:\\ComfyUI-Alpha",
  norm: "C:\\ComfyUI-Beta",
};

function buildInstalls(): Install[] {
  return [
    {
      id: "prod",
      label: "Production",
      registeredPath: INSTALL_ROOTS.prod!,
      root: INSTALL_ROOTS.prod!,
      modelsDir: `${INSTALL_ROOTS.prod}\\models`,
      version: "0.29.1",
      versionSource: "comfyui_version.py",
      extraPaths: [],
      outputModelDirs: [],
      addedAt: "2026-09-11T10:04:00.000Z",
      lastScanAt: null,
      lastScanTotals: null,
    },
    {
      id: "norm",
      label: "Normal",
      registeredPath: INSTALL_ROOTS.norm!,
      root: INSTALL_ROOTS.norm!,
      modelsDir: `${INSTALL_ROOTS.norm}\\models`,
      version: "0.27.4",
      versionSource: "pyproject.toml",
      extraPaths: [
        {
          section: "comfyui",
          category: "checkpoints",
          rawCategory: "checkpoints",
          path: "D:\\ai-models\\ltx",
          isDefault: false,
          exists: true,
        },
      ],
      outputModelDirs: [],
      addedAt: "2026-09-11T16:22:00.000Z",
      lastScanAt: null,
      lastScanTotals: null,
    },
  ];
}

function parseCopy(raw: string, filename: string): Copy {
  let s = raw;
  let blocked: BlockReason | null = null;
  let name = filename;

  const flag = /\s!(\w+)$/.exec(s);
  if (flag) {
    blocked = BLOCK_FLAGS[flag[1]!] ?? null;
    s = s.slice(0, flag.index);
  }
  const alias = /\s>(\S+)$/.exec(s);
  if (alias) {
    name = alias[1]!;
    s = s.slice(0, alias.index);
  }
  const colon = s.indexOf(":");
  const installId = s.slice(0, colon);
  const folder = s.slice(colon + 1);
  const absolute = /^[A-Za-z]:\\/.test(folder);
  const root = INSTALL_ROOTS[installId] ?? "C:\\";
  const absPath = absolute ? folder + name : `${root}\\${folder}${name}`;
  return {
    installId,
    folder,
    name,
    absPath,
    relPath: absolute ? absPath : `${folder}${name}`,
    volume: absPath.slice(0, 2).toUpperCase(),
    isLink: false,
    blocked,
  };
}

export function buildWorld(): World {
  const contents: Content[] = ROWS.map((row, index) => {
    const [filename, category, mb, workflowHits, copies, civitai] = row;
    const sha256 = fakeSha(`${filename}|${mb}|${index}`);
    return {
      sha256,
      filename,
      category,
      bytes: mb * MB,
      workflowHits,
      copies: copies.map((c) => parseCopy(c, filename)),
      metadata: civitai
        ? {
            sha256,
            source: "civitai",
            fetchedAt: "2026-09-22T12:00:00.000Z",
            found: true,
            modelName: civitai[0],
            versionName: civitai[1],
            modelType: civitai[2],
            baseModel: civitai[3],
            triggerWords: [],
            nsfw: false,
            nsfwLevel: 0,
            civitaiModelId: 1000 + index,
            civitaiVersionId: 5000 + index,
            pageUrl: `https://civitai.com/models/${1000 + index}`,
            downloadUrl: null,
            previewImageUrls: [],
            ambiguous: false,
          }
        : null,
    };
  });

  const vault = new Map<string, VaultEntry>();
  for (const content of contents) {
    if (content.copies.length > 0) continue;
    vault.set(content.sha256, {
      sha256: content.sha256,
      canonicalName: content.filename,
      aliases: [],
      addedAt: "2026-09-12T18:40:00.000Z",
    });
  }

  return {
    installs: buildInstalls(),
    contents,
    vault,
    links: [],
    freeBytes: VAULT_FREE_BYTES,
    running: ["prod"],
    symlinksSupported: false,
    metadataLookupsEnabled: true,
    workflowsOnDisk: 42,
  };
}

// ── derivations ─────────────────────────────────────────────────────────────

/** Weights that are counted and never moved. These are not a problem. */
export const COUNTED_NEVER_MOVED = {
  customNodeFiles: 14,
  customNodeBytes: 25190 * MB,
  hfCacheFiles: 212,
  hfCacheBytes: 91136 * MB,
};

function movableCopies(world: World): Array<{ content: Content; copy: Copy }> {
  const out: Array<{ content: Content; copy: Copy }> = [];
  for (const content of world.contents) {
    for (const copy of content.copies) out.push({ content, copy });
  }
  return out;
}

export function scanTotalsOf(
  world: World,
  installId?: string,
): ScanTotals {
  const rows = movableCopies(world).filter(
    (r) => !installId || r.copy.installId === installId,
  );
  const seenContents = new Set<string>();
  let movableFiles = 0;
  let movableBytes = 0;
  let uniqueBytes = 0;
  let duplicateFiles = 0;
  let alreadyLinkedFiles = 0;
  let alreadyLinkedBytes = 0;

  const perContent = new Map<string, number>();
  for (const { content, copy } of rows) {
    if (copy.isLink) {
      alreadyLinkedFiles += 1;
      alreadyLinkedBytes += content.bytes;
      continue;
    }
    movableFiles += 1;
    movableBytes += content.bytes;
    perContent.set(content.sha256, (perContent.get(content.sha256) ?? 0) + 1);
    if (!seenContents.has(content.sha256)) {
      seenContents.add(content.sha256);
      uniqueBytes += content.bytes;
    }
  }
  for (const [sha, count] of perContent) {
    if (count > 1) duplicateFiles += count - 1;
    void sha;
  }

  const shared = installId ? { customNodeFiles: 0, customNodeBytes: 0, hfCacheFiles: 0, hfCacheBytes: 0 } : COUNTED_NEVER_MOVED;

  return {
    filesSeen: rows.length + shared.customNodeFiles + shared.hfCacheFiles,
    movableFiles,
    movableBytes,
    uniqueContents: seenContents.size,
    uniqueBytes,
    reclaimableBytes: movableBytes - uniqueBytes,
    duplicateFiles,
    alreadyLinkedFiles,
    alreadyLinkedBytes,
    customNodeFiles: shared.customNodeFiles,
    customNodeBytes: shared.customNodeBytes,
    hfCacheFiles: shared.hfCacheFiles,
    hfCacheBytes: shared.hfCacheBytes,
    skippedFiles: 0,
    errorCount: 0,
    bytesRead: movableBytes,
    bytesFromCache: 0,
    durationMs: 214_000,
  };
}

export function scanResultOf(world: World, scanId: string, cancelled = false): ScanResult {
  const perInstall: InstallScanTotals[] = world.installs.map((install) => ({
    ...scanTotalsOf(world, install.id),
    installId: install.id,
    installLabel: install.label,
  }));
  return {
    scanId,
    startedAt: new Date(Date.now() - 214_000).toISOString(),
    finishedAt: new Date().toISOString(),
    installIds: world.installs.map((i) => i.id),
    cancelled,
    totals: scanTotalsOf(world),
    perInstall,
    errors: [],
  };
}

export function scanEntriesOf(world: World): ScanEntry[] {
  return movableCopies(world).map(({ content, copy }) => ({
    absPath: copy.absPath,
    relPath: copy.relPath,
    installId: copy.installId,
    category: content.category,
    sizeBytes: content.bytes,
    sha256: content.sha256,
    modifiedAt: "2026-09-10T08:00:00.000Z",
    classification: copy.isLink ? "alreadyInVault" : "movable",
    occurrenceCount: content.copies.length,
    linkTarget: copy.isLink
      ? `${VAULT_ROOT}\\${content.category}\\${content.filename}`
      : null,
  }));
}

/** "lora1.safetensors" + a hash -> "lora1__3F9A2C17.safetensors" */
function adjustedName(filename: string, sha256: string): string {
  const dot = filename.lastIndexOf(".");
  const suffix = `__${sha256.slice(0, 8)}`;
  if (dot <= 0) return filename + suffix;
  return filename.slice(0, dot) + suffix + filename.slice(dot);
}

/**
 * The plan, built by the rules in docs/IPC-CONTRACT.md section 5.3: one group
 * per unique content, the vault path is the category plus the file name, the
 * first content to want a name keeps it, and the copy that becomes the vault
 * file is one already on the vault volume when there is one.
 */
export function planOf(world: World, planId: string, scanId: string): ConsolidationPlan {
  const groups: PlanGroup[] = [];
  const blocked: BlockedRow[] = [];
  const takenNames = new Map<string, string>();

  const installLabel = (id: string) =>
    world.installs.find((i) => i.id === id)?.label ?? id;

  for (const content of world.contents) {
    for (const copy of content.copies) {
      if (!copy.blocked) continue;
      blocked.push({
        absPath: copy.absPath,
        installId: copy.installId,
        installLabel: installLabel(copy.installId),
        sizeBytes: content.bytes,
        sha256: content.sha256,
        reason: copy.blocked,
        detail: `${copy.absPath} could not be prepared.`,
      });
    }

    const usable = content.copies.filter((c) => !c.blocked && !c.isLink);
    if (usable.length === 0) continue;

    // The plan is still built when links are unavailable, so the person can read
    // what would happen before turning Developer Mode on. One blocked row says
    // so, added below: the reason is a fact about the computer, not any file.
    const sorted = [...usable].sort((a, b) => a.absPath.localeCompare(b.absPath));
    const onVaultVolume = sorted.find((c) => c.volume === VAULT_VOLUME);
    const sourceCopy = onVaultVolume ?? sorted[0]!;
    const chosenBecause: PlanSource["chosenBecause"] =
      sorted.length === 1
        ? "onlyCopy"
        : onVaultVolume
          ? "sameVolume"
          : "firstByPath";

    const owner = takenNames.get(content.filename);
    const adjusted = owner !== undefined && owner !== content.sha256;
    const vaultName = adjusted
      ? adjustedName(content.filename, content.sha256)
      : content.filename;
    if (!adjusted) takenNames.set(content.filename, content.sha256);

    const source: PlanSource = {
      installId: sourceCopy.installId,
      installLabel: installLabel(sourceCopy.installId),
      absPath: sourceCopy.absPath,
      relPath: sourceCopy.relPath,
      sameVolumeAsVault: sourceCopy.volume === VAULT_VOLUME,
      chosenBecause,
    };

    // Every place that held the file gets a link, the one the bytes move out
    // of included, so this is always `occurrences` long.
    const links: PlanLink[] = sorted.map((copy) => ({
      installId: copy.installId,
      installLabel: installLabel(copy.installId),
      absPath: copy.absPath,
      relPath: copy.relPath,
      linkName: copy.name,
      nameDiffersFromVault: copy.name !== vaultName,
      isSource: copy.absPath === sourceCopy.absPath,
    }));

    groups.push({
      groupId: `g-${content.sha256.slice(0, 12)}`,
      sha256: content.sha256,
      sizeBytes: content.bytes,
      category: content.category,
      vaultRelPath: `${content.category}/${vaultName}`,
      vaultNameAdjusted: adjusted,
      clashesWith: adjusted ? (owner ?? null) : null,
      source,
      links,
      occurrences: sorted.length,
      bytesFreed: (sorted.length - 1) * content.bytes,
      singleCopy: sorted.length === 1,
      crossVolume: sorted.some((c) => c.volume !== VAULT_VOLUME),
    });
  }

  if (!world.symlinksSupported) {
    blocked.push({
      absPath: VAULT_ROOT,
      installId: null,
      installLabel: null,
      sizeBytes: 0,
      sha256: null,
      reason: "symlinkUnsupported",
      detail: "This computer cannot create symbolic links right now.",
    });
  }

  for (const kind of ["inCustomNodes", "inHuggingFaceCache"] as const) {
    blocked.push({
      absPath:
        kind === "inCustomNodes"
          ? `${INSTALL_ROOTS.prod}\\custom_nodes`
          : "C:\\Users\\alex\\.cache\\huggingface\\hub",
      installId: kind === "inCustomNodes" ? "prod" : null,
      installLabel: kind === "inCustomNodes" ? "Production" : null,
      sizeBytes:
        kind === "inCustomNodes"
          ? COUNTED_NEVER_MOVED.customNodeBytes
          : COUNTED_NEVER_MOVED.hfCacheBytes,
      sha256: null,
      reason: kind,
      detail: "Counted, never moved.",
    });
  }

  const bytesFreed = groups.reduce((s, g) => s + g.bytesFreed, 0);
  const listed = blocked.filter(
    (b) =>
      b.reason !== "inCustomNodes" &&
      b.reason !== "inHuggingFaceCache" &&
      b.reason !== "symlinkUnsupported",
  );

  return {
    planId,
    scanId,
    createdAt: new Date().toISOString(),
    vaultRoot: VAULT_ROOT,
    symlinksSupported: world.symlinksSupported,
    groups,
    blocked,
    totals: {
      groups: groups.length,
      groupsFreeingSpace: groups.filter((g) => g.bytesFreed > 0).length,
      singleCopyGroups: groups.filter((g) => g.singleCopy).length,
      nameClashes: groups.filter((g) => g.vaultNameAdjusted).length,
      crossVolumeGroups: groups.filter((g) => g.crossVolume).length,
      bytesFreed,
      bytesMoved: groups.reduce((s, g) => s + g.sizeBytes, 0),
      filesMoved: groups.length,
      linksCreated: groups.reduce((s, g) => s + g.occurrences, 0),
      blockedRows: listed.length,
      blockedBytes: listed.reduce((s, b) => s + b.sizeBytes, 0),
      vaultFreeBytesAfter: world.freeBytes + bytesFreed,
    },
  };
}

export function vaultFilesOf(world: World): VaultFile[] {
  return [...world.vault.values()].map((entry) => {
    const content = world.contents.find((c) => c.sha256 === entry.sha256);
    const links = world.links.filter((l) => l.sha256 === entry.sha256);
    return {
      sha256: entry.sha256,
      canonicalName: entry.canonicalName,
      category: content?.category ?? "",
      vaultRelPath: `${content?.category ?? ""}/${entry.canonicalName}`,
      sizeBytes: content?.bytes ?? 0,
      addedAt: entry.addedAt,
      aliases: entry.aliases,
      linkCount: links.length,
      links,
      metadata: world.metadataLookupsEnabled ? (content?.metadata ?? null) : null,
      present: true,
    };
  });
}

/** One row per unique content, across the vault and the installs. */
export function contentRowsOf(world: World): ContentRow[] {
  const rows: ContentRow[] = [];
  for (const content of world.contents) {
    const entry = world.vault.get(content.sha256);
    const links = world.links.filter((l) => l.sha256 === content.sha256);
    if (content.copies.length === 0 && !entry) continue;
    rows.push({
      sha256: content.sha256,
      name: entry?.canonicalName ?? content.filename,
      category: content.category,
      sizeBytes: content.bytes,
      aliases: entry?.aliases ?? [
        ...new Set(
          content.copies.map((c) => c.name).filter((n) => n !== content.filename),
        ),
      ],
      occurrenceCount: content.copies.length,
      linkCount: links.length,
      inVault: entry !== undefined,
      installIds: [...new Set(content.copies.map((c) => c.installId))],
      addedAt: entry?.addedAt ?? null,
      metadata: world.metadataLookupsEnabled ? content.metadata : null,
    });
  }
  return rows;
}

export function nameGroupsOf(world: World): NameGroup[] {
  const out: NameGroup[] = [];
  for (const entry of world.vault.values()) {
    if (entry.aliases.length === 0) continue;
    const content = world.contents.find((c) => c.sha256 === entry.sha256);
    if (!content) continue;
    const names = [entry.canonicalName, ...entry.aliases];
    out.push({
      sha256: entry.sha256,
      sizeBytes: content.bytes,
      category: content.category,
      canonicalName: entry.canonicalName,
      names: names.map((name) => ({
        name,
        isCanonical: name === entry.canonicalName,
        vaultRelPath: `${content.category}/${name}`,
        usedByLinks: world.links.filter(
          (l) => l.sha256 === entry.sha256 && l.linkName === name,
        ).length,
        seenInInstalls: [
          ...new Set(
            content.copies
              .filter((c) => c.name === name)
              .map((c) => c.installId),
          ),
        ],
      })),
    });
  }
  return out;
}
