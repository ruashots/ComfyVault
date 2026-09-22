/**
 * Figures that were settled when the person approved the mock and do not depend
 * on any engine rule: how a size reads on screen.
 *
 * The mock's totals are NOT here. They were produced under the mock's own
 * grouping rules, and docs/IPC-CONTRACT.md settles those rules differently: a
 * copy on another drive is copied across rather than left behind, and the copy
 * that becomes the vault file is the one already on the vault's drive. The
 * engine's rules are what happen on disk, so they are what the tests pin.
 */

export const MB = 1024 * 1024;

/** What each of these sizes reads as on screen. */
export const SIZE_STRINGS: ReadonlyArray<readonly [number, string]> = [
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
];
