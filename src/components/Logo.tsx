import large from "~/assets/brand/logo.png";
import small from "~/assets/brand/logo-small.png";

/**
 * The ComfyVault logo: the mark and the word, made for a dark background.
 *
 * Two copies, each drawn at twice the height it is shown at, so the thin lines
 * stay sharp on a high-density screen and the title bar never scales a large
 * picture down by five.
 */
export function Logo(props: { height: 18 | 48 }) {
  return (
    <img
      class="logo"
      src={props.height === 18 ? small : large}
      height={props.height}
      alt="ComfyVault"
      draggable={false}
      data-tauri-drag-region
    />
  );
}
