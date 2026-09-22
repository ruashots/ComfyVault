/**
 * The figures the person approved.
 *
 * They were produced by running the derivation inside
 * design/mock/comfyvault.html, which is the blessed mock, and reading its
 * totals. The app must print the same numbers from the same data. If a rule in
 * src/domain/plan.ts changes, these tests fail and the change has to be
 * defended.
 *
 * Everything here is in megabytes, the unit the mock counts in. Multiply by MB
 * to compare against the app, which counts in bytes.
 */

export const MB = 1024 * 1024;

export const BLESSED = {
  models: 97,
  files: 172,
  uniqueMB: 797350,
  onDiskMB: 1542224,
  reclaimMB: 614673,
  duplicateCopies: 70,
  blockedMB: 130201,
  vaultOnlyMB: 7645,
  unused: 35,
  countedNeverMovedMB: 116326,

  duplicateGroups: 68,
  clashGroups: 4,
  singles: 18,
  singlesMB: 168714,
  blocked: 8,
  orphans: 3,
  aliasGroups: 5,

  /** What closing ComfyUI adds to the run. */
  comfyCostMB: 57691,

  perInstance: {
    prod: { mb: 782551, files: 88, moving: 85, movingMB: 724860, stuck: 3, stuckMB: 57691 },
    norm: { mb: 752028, files: 84, moving: 79, movingMB: 679518, stuck: 5, stuckMB: 72510 },
  },

  /** Every model ticked. */
  selectionAll: {
    mb: 614673,
    groups: 68,
    moves: 94,
    links: 164,
    duplicateCopies: 70,
  },

  /** Same filename, different bytes: which one keeps the plain name. */
  clashNames: [
    {
      filename: "diffusion_pytorch_model.safetensors",
      vaultNames: [
        "diffusion_pytorch_model.safetensors",
        "diffusion_pytorch_model-2.safetensors",
      ],
      mb: [2508, 1409],
    },
    {
      filename: "vae.safetensors",
      vaultNames: ["vae.safetensors", "vae-2.safetensors"],
      mb: [351, 319],
    },
    {
      filename: "model.safetensors",
      vaultNames: ["model.safetensors", "model-2.safetensors"],
      mb: [1246, 812],
    },
    {
      filename: "pytorch_lora_weights.safetensors",
      vaultNames: [
        "pytorch_lora_weights.safetensors",
        "pytorch_lora_weights-2.safetensors",
      ],
      mb: [689, 402],
    },
  ],

  /** Grouped by reason, biggest first inside each group. */
  blockedOrder: [
    { filename: "ltxv-13b-0.9.7-dev.safetensors", kind: "other_drive", mb: 27034 },
    { filename: "ltxv-13b-0.9.8-distilled.safetensors", kind: "other_drive", mb: 13517 },
    { filename: "ltx-video-2b-v0.9.5.safetensors", kind: "other_drive", mb: 9421 },
    { filename: "ltxv-vae-0.9.7.safetensors", kind: "other_drive", mb: 1649 },
    { filename: "hunyuan_video_t2v_720p_bf16.safetensors", kind: "file_open", mb: 26214 },
    { filename: "flux1-dev.safetensors", kind: "file_open", mb: 24371 },
    { filename: "sd_xl_base_1.0.safetensors", kind: "file_open", mb: 7106 },
    { filename: "qwen_image_fp8_e4m3fn.safetensors", kind: "permission_denied", mb: 20889 },
  ],

  /** The ten biggest wins, with the copy that moves into the vault. */
  topDuplicates: [
    { filename: "wan2.1_i2v_480p_14B_bf16.safetensors", reclaimMB: 33587, keeper: "prod", folder: "models\\diffusion_models\\bf16\\" },
    { filename: "hunyuan_video_i2v_720p_bf16.safetensors", reclaimMB: 26214, keeper: "prod", folder: "models\\diffusion_models\\" },
    { filename: "flux1-dev.safetensors", reclaimMB: 24371, keeper: "prod", folder: "models\\checkpoints\\flux\\" },
    { filename: "flux1-schnell.safetensors", reclaimMB: 24371, keeper: "prod", folder: "models\\diffusion_models\\" },
    { filename: "flux1-fill-dev.safetensors", reclaimMB: 24371, keeper: "prod", folder: "models\\diffusion_models\\" },
    { filename: "flux1-krea-dev.safetensors", reclaimMB: 24371, keeper: "prod", folder: "models\\diffusion_models\\" },
    { filename: "flux1-kontext-dev.safetensors", reclaimMB: 24371, keeper: "prod", folder: "models\\diffusion_models\\" },
    { filename: "qwen_image_edit_2509_fp8.safetensors", reclaimMB: 20889, keeper: "prod", folder: "models\\diffusion_models\\" },
    { filename: "qwen_image_edit_fp8_e4m3fn.safetensors", reclaimMB: 20889, keeper: "prod", folder: "models\\diffusion_models\\" },
    { filename: "qwen_image_distill_full_fp8.safetensors", reclaimMB: 20889, keeper: "prod", folder: "models\\diffusion_models\\" },
  ],

  /** What each of these sizes reads as on screen. */
  sizeStrings: [
    [614673, "600 GB"],
    [1542224, "1.47 TB"],
    [797350, "779 GB"],
    [139264, "136 GB"],
    [1908408, "1.82 TB"],
    [241, "241 MB"],
    [66, "66 MB"],
    [5, "5 MB"],
    [1024, "1.0 GB"],
    [10240, "10 GB"],
    [1048576, "1.00 TB"],
  ] as ReadonlyArray<readonly [number, string]>,

  folders: [
    "checkpoints",
    "clip_vision",
    "controlnet",
    "diffusion_models",
    "loras",
    "text_encoders",
    "upscale_models",
    "vae",
  ],
} as const;
