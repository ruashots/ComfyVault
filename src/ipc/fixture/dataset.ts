/**
 * The development dataset.
 *
 * Every row is copied from the blessed mock at design/mock/comfyvault.html, so
 * the numbers the interface prints can be compared against it line by line.
 * Nothing here ever reads a real ComfyUI install.
 *
 * Row: [filename, folder, megabytes, workflowHits, copies, civitai?]
 * Copy: "instanceId:folder" + optional " >filenameUsedThere" + optional " !open|drive|perm"
 */

import { derivePlan } from "~/domain/plan";
import { fmt } from "~/domain/format";
import type {
  ActivityEntry,
  BlockedReason,
  CountedNeverMoved,
  Instance,
  MachineState,
  Model,
  Placement,
  RemovedInstance,
  ScanResult,
} from "~/ipc/contract";

const MB = 1024 * 1024;

type Row = [
  filename: string,
  folder: string,
  mb: number,
  workflowHits: number,
  copies: string[],
  civitai?: [name: string, version: string, type: string, base: string, by: string],
];

export const FIXTURE_INSTANCES: Instance[] = [
  {
    id: "prod",
    name: "Production",
    path: "C:\\ComfyUI-Alpha",
    running: true,
    extraModelPaths: null,
    addedAt: "2026-09-11T10:04:00",
  },
  {
    id: "norm",
    name: "Normal",
    path: "C:\\ComfyUI-Beta",
    running: false,
    extraModelPaths: ["D:\\ai-models\\"],
    addedAt: "2026-09-11T16:22:00",
  },
];

export const FIXTURE_REMOVED_INSTANCE: RemovedInstance = {
  name: "ComfyUI-Portable",
  path: "C:\\ComfyUI-Portable",
  removedAt: "2026-09-14T09:13:00",
};

export const FIXTURE_COUNTED_NEVER_MOVED: CountedNeverMoved[] = [
  { kind: "custom_nodes", bytes: 25190 * MB, where: "14 folders across 2 installs" },
  {
    kind: "huggingface_cache",
    bytes: 91136 * MB,
    where: "C:\\Users\\alex\\.cache\\huggingface\\hub",
  },
];

export const FIXTURE_DRIVE = {
  letter: "C:",
  totalBytes: 1908408 * MB,
  freeBytes: 139264 * MB,
};

/** The one ComfyUI holding files open in the fixture. */
export const FIXTURE_COMFY_PROCESS = {
  instanceId: "prod",
  process: "python.exe",
  pid: 18244,
  openFiles: 3,
};

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
  ["flux1-dev.safetensors","diffusion_models",24371,8,["prod:models\\diffusion_models\\ !open","prod:models\\checkpoints\\flux\\","norm:models\\unet\\"]],
  ["flux1-schnell.safetensors","diffusion_models",24371,1,["prod:models\\diffusion_models\\","norm:models\\unet\\"]],
  ["flux1-fill-dev.safetensors","diffusion_models",24371,2,["prod:models\\diffusion_models\\","norm:models\\unet\\"]],
  ["flux1-krea-dev.safetensors","diffusion_models",24371,3,["prod:models\\diffusion_models\\","norm:models\\unet\\"]],
  ["flux1-kontext-dev.safetensors","diffusion_models",24371,6,["prod:models\\diffusion_models\\","norm:models\\unet\\"]],
  ["flux1-dev-fp8.safetensors","checkpoints",12186,0,["prod:models\\checkpoints\\","norm:models\\checkpoints\\"]],
  ["hunyuan_video_t2v_720p_bf16.safetensors","diffusion_models",26214,2,["prod:models\\diffusion_models\\ !open","norm:models\\diffusion_models\\hunyuan\\"]],
  ["hunyuan_video_i2v_720p_bf16.safetensors","diffusion_models",26214,1,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\hunyuan\\"]],
  ["hunyuan_video_t2v_720p_fp8_scaled.safetensors","diffusion_models",13107,0,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\hunyuan\\"]],
  ["hunyuan_video_i2v_720p_fp8_scaled.safetensors","diffusion_models",13107,0,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\hunyuan\\"]],
  ["cosmos-1_0-diffusion-7b-text2world.safetensors","diffusion_models",14541,0,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\cosmos\\"]],
  ["cosmos_predict2_2B_t2i.safetensors","diffusion_models",4403,0,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\cosmos\\"]],
  ["mochi_preview_bf16.safetensors","diffusion_models",20889,0,["norm:models\\diffusion_models\\"]],
  ["qwen_image_fp8_e4m3fn.safetensors","diffusion_models",20889,6,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\qwen\\ !perm"]],
  ["qwen_image_edit_2509_fp8.safetensors","diffusion_models",20889,5,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\qwen\\"]],
  ["qwen_image_edit_fp8_e4m3fn.safetensors","diffusion_models",20889,2,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\qwen\\"]],
  ["qwen_image_distill_full_fp8.safetensors","diffusion_models",20889,1,["prod:models\\diffusion_models\\","norm:models\\diffusion_models\\qwen\\"]],
  ["hidream_i1_full_fp8.safetensors","diffusion_models",17510,0,["prod:models\\diffusion_models\\"]],
  ["svd_xt_1_1.safetensors","checkpoints",9789,0,["prod:models\\checkpoints\\"]],
  ["sd3.5_large.safetensors","checkpoints",16896,2,["prod:models\\checkpoints\\","norm:models\\checkpoints\\sd35\\"]],
  ["sd3.5_large_turbo.safetensors","checkpoints",16896,0,["prod:models\\checkpoints\\","norm:models\\checkpoints\\sd35\\"]],
  ["sd3.5_medium.safetensors","checkpoints",5222,0,["prod:models\\checkpoints\\","norm:models\\checkpoints\\sd35\\"]],
  ["sd_xl_base_1.0.safetensors","checkpoints",7106,4,["prod:models\\checkpoints\\","prod:models\\checkpoints\\sdxl\\ !open","norm:models\\checkpoints\\"]],
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
  // same filename, different bytes
  ["diffusion_pytorch_model.safetensors","controlnet",2508,1,["prod:models\\controlnet\\union\\"]],
  ["diffusion_pytorch_model.safetensors","controlnet",1409,0,["norm:models\\controlnet\\openpose\\"]],
  ["vae.safetensors","vae",351,2,["prod:models\\vae\\"]],
  ["vae.safetensors","vae",319,1,["norm:models\\vae\\"]],
  ["model.safetensors","clip_vision",1246,0,["prod:models\\clip_vision\\"]],
  ["model.safetensors","clip_vision",812,1,["norm:models\\clip_vision\\sigclip\\"]],
  ["pytorch_lora_weights.safetensors","loras",689,0,["prod:models\\loras\\flux\\"]],
  ["pytorch_lora_weights.safetensors","loras",402,1,["norm:models\\loras\\"]],
  // already in the vault, nothing links to them (ComfyUI-Portable was removed)
  ["sd15_inpainting_v1.5.safetensors","checkpoints",4068,0,[]],
  ["deliberate_v2.safetensors","checkpoints",2132,0,[],["Deliberate","v2","Checkpoint","SD 1.5","XpucT"]],
  ["control_v11f1p_sd15_depth.pth","controlnet",1445,0,[]],
];

/** A stable 64 character stand-in for a real SHA-256, so ids never move. */
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
  return out;
}

const BLOCK_FLAGS: Record<string, BlockedReason> = {
  open: {
    kind: "file_open",
    process: FIXTURE_COMFY_PROCESS.process,
    pid: FIXTURE_COMFY_PROCESS.pid,
    instanceId: FIXTURE_COMFY_PROCESS.instanceId,
  },
  drive: { kind: "other_drive", drive: "D:", vaultDrive: "C:" },
  perm: { kind: "permission_denied" },
};

function instancePath(id: string): string {
  return FIXTURE_INSTANCES.find((i) => i.id === id)?.path ?? "C:\\";
}

function parseCopy(
  raw: string,
  filename: string,
  key: string,
): Placement {
  let s = raw;
  let blocked: BlockedReason | null = null;
  let usedName = filename;

  const flag = /\s!(\w+)$/.exec(s);
  if (flag) {
    blocked = BLOCK_FLAGS[flag[1]!] ?? null;
    s = s.slice(0, flag.index);
  }
  const alias = /\s>(\S+)$/.exec(s);
  if (alias) {
    usedName = alias[1]!;
    s = s.slice(0, alias.index);
  }
  const colon = s.indexOf(":");
  const instanceId = s.slice(0, colon);
  const folder = s.slice(colon + 1);
  const absolute = /^[A-Za-z]:\\/.test(folder);
  const base = absolute ? folder : `${instancePath(instanceId)}\\${folder}`;

  return {
    id: key,
    instanceId,
    folder,
    filename: usedName,
    fullPath: base + usedName,
    isLink: false,
    blocked,
  };
}

function buildModels(): Model[] {
  return ROWS.map((row, index) => {
    const [filename, folder, mb, workflowHits, copies, civitai] = row;
    const bytes = mb * MB;
    const sha = fakeSha(`${filename}|${mb}|${index}`);
    const placements = copies.map((c, ci) =>
      parseCopy(c, filename, `${index}-${ci}`),
    );
    const byName: Record<string, number> = { [filename]: workflowHits };
    for (const p of placements) {
      if (p.filename !== filename && byName[p.filename] === undefined) {
        byName[p.filename] = Math.max(0, workflowHits - 2);
      }
    }
    return {
      id: sha,
      sha256: sha,
      filename,
      folder,
      bytes,
      placements,
      workflowHits,
      workflowHitsByName: byName,
      civitai: civitai
        ? {
            name: civitai[0],
            version: civitai[1],
            type: civitai[2],
            baseModel: civitai[3],
            uploader: civitai[4],
          }
        : null,
      inVaultSince: placements.length === 0 ? "2026-09-12T18:40:00" : null,
    };
  });
}

export const FIXTURE_MODELS: Model[] = buildModels();

export function fixtureMachine(overrides: Partial<MachineState> = {}): MachineState {
  return {
    developerMode: false,
    running: [FIXTURE_COMFY_PROCESS],
    vaultPath: "C:\\ComfyVault",
    vaultDrive: { ...FIXTURE_DRIVE },
    ...overrides,
  };
}

function fixtureActivity(
  models: Model[],
  scannedAt: string,
  totals: { duplicateCopies: number; reclaimBytes: number },
): ActivityEntry[] {
  return [
    {
      event: "vault.scan.complete",
      detail: `${models.length} models \u00b7 ${totals.duplicateCopies} duplicate copies \u00b7 ${fmt(totals.reclaimBytes)} reclaimable`,
      when: scannedAt,
    },
    {
      event: "instance.remove",
      detail: "ComfyUI-Portable \u00b7 3 vault files left with nothing pointing at them",
      when: FIXTURE_REMOVED_INSTANCE.removedAt,
    },
    {
      event: "vault.scan.complete",
      detail: "96 models \u00b7 64 duplicate copies \u00b7 548 GB reclaimable",
      when: "2026-09-12T19:02:00",
    },
    {
      event: "instance.add",
      detail: "Normal \u00b7 C:\\ComfyUI-Beta",
      when: "2026-09-11T16:22:00",
    },
    {
      event: "vault.create",
      detail: "C:\\ComfyVault \u00b7 drive C: \u00b7 empty",
      when: "2026-09-11T10:06:00",
    },
  ];
}

export function fixtureScan(models: Model[] = FIXTURE_MODELS): ScanResult {
  const scannedAt = new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString();
  const scan: ScanResult = {
    scannedAt,
    instances: FIXTURE_INSTANCES.map((i) => ({ ...i })),
    models,
    countedNeverMoved: FIXTURE_COUNTED_NEVER_MOVED.map((x) => ({ ...x })),
    removedInstance: { ...FIXTURE_REMOVED_INSTANCE },
    activity: [],
    civitaiEnabled: true,
  };
  // The newest log line repeats what the scan found, so it reads the same as
  // the figures on the rest of the screen.
  scan.activity = fixtureActivity(
    models,
    scannedAt,
    derivePlan(scan, fixtureMachine()).totals,
  );
  return scan;
}
