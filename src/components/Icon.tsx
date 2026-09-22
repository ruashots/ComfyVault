/**
 * The icon set, drawn for this interface. Monoline, 24x24, stroke 1.45, taking
 * the colour of the text around it. No icon library.
 *
 * The path data below is copied from design/mock/comfyvault.html. It is a fixed
 * set of constants in this file, never anything a scan produced.
 */

const ICONS = {
  home: '<path d="M4 17a8 8 0 0 1 16 0"/><path d="M12 17 16.4 11.6"/><circle cx="12" cy="17" r="1.1" fill="currentColor" stroke="none"/><path d="M4 17h2"/><path d="M18 17h2"/>',
  library:
    '<path d="M3.5 5.5h4v3.5h-4z"/><path d="M9.5 7.25h11"/><path d="M3.5 10.25h4v3.5h-4z"/><path d="M9.5 12h11"/><path d="M3.5 15h4v3.5h-4z"/><path d="M9.5 16.75h11"/>',
  consolidate: '<path d="M3.2 4.8h17.6L14.3 12.7v5.6l-4.6 2.6v-8.2z"/>',
  cleanup:
    '<path d="M12.5 3.5h8v8l-9 9-8-8z"/><circle cx="16.6" cy="7.4" r="1.35"/>',
  download: '<path d="M12 4v10.5"/><path d="M7.5 10.5 12 15l4.5-4.5"/><path d="M4 18.5h16"/>',
  settings:
    '<path d="M3.5 8h17"/><path d="M3.5 16.5h17"/><path d="M7 5.9h3.2v4.2H7z"/><path d="M13.8 14.4H17v4.2h-3.2z"/>',
  scan: '<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="3.8"/><path d="M12 12 17.6 6.4"/><circle cx="12" cy="12" r=".9" fill="currentColor" stroke="none"/>',
  link: '<path d="M9.8 14.2a3.8 3.8 0 0 1 0-5.4l2.6-2.6a3.8 3.8 0 1 1 5.4 5.4l-1.3 1.3"/><path d="M14.2 9.8a3.8 3.8 0 0 1 0 5.4l-2.6 2.6a3.8 3.8 0 1 1-5.4-5.4l1.3-1.3"/>',
  check: '<path d="M5 12.5 10 17.5 19 6.5"/>',
  x: '<path d="M6.5 6.5 17.5 17.5"/><path d="M17.5 6.5 6.5 17.5"/>',
  warn: '<path d="M12 3.6 21.6 20H2.4z"/><path d="M12 9.6v4.8"/><path d="M12 17.4h.01"/>',
  folder: '<path d="M3 6.5h6.2l2 2.6H21v9.4H3z"/>',
  drive: '<path d="M3.5 5.5h17v13h-17z"/><path d="M3.5 12.2h17"/><path d="M6.4 15.6h2.2"/>',
  vault:
    '<path d="M9 4.2h11v15.6H9z"/><path d="M2.4 6.6 6.4 12"/><path d="M2.4 12h4"/><path d="M2.4 17.4 6.4 12"/><path d="M6.4 12H9"/><path d="M12.6 9.2h4.2v5.6h-4.2z" fill="currentColor" stroke="none"/>',
  arrow: '<path d="M4.5 12h14"/><path d="M13 6.5 18.5 12 13 17.5"/>',
  search: '<circle cx="10.6" cy="10.6" r="6.1"/><path d="M15.2 15.2 20 20"/>',
  chev: '<path d="M6.5 9.5 12 15.2 17.5 9.5"/>',
  plus: '<path d="M12 5v14"/><path d="M5 12h14"/>',
  trash: '<path d="M4.5 7h15"/><path d="M9.5 7V4.5h5V7"/><path d="m6.8 7 1 12.5h8.4L17 7"/>',
  refresh: '<path d="M20 12a8 8 0 1 1-2.6-5.9"/><path d="M20 4.4V9h-4.6"/>',
  dot: '<circle cx="12" cy="12" r="2.4" fill="currentColor" stroke="none"/>',
  clock: '<circle cx="12" cy="12" r="8.2"/><path d="M12 7.2V12l3.2 2"/>',
  stop: '<path d="M7.5 7.5h9v9h-9z"/>',
  file: '<path d="M5.5 3.5h8.4L18.5 8v12.5h-13z"/><path d="M13.5 3.8V8.3h4.7"/>',
  external:
    '<path d="M18.5 13.2v6.3H4.5V5.5h6.3"/><path d="M14 4.5h5.5V10"/><path d="M10.5 13.5 19.5 4.5"/>',
  minus: '<path d="M5 12h14"/>',
  square: '<path d="M5 5h14v14H5z"/>',
  win: '<path d="M6 8h12v10H6z"/><path d="M6 8V6h12v2"/>',
} as const;

export type IconName = keyof typeof ICONS;

export function Icon(props: { name: IconName; size?: number; class?: string }) {
  return (
    <svg
      width={props.size ?? 14}
      height={props.size ?? 14}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.45"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      class={props.class}
      innerHTML={ICONS[props.name]}
    />
  );
}

/** The wordmark's vault, the one place the amber is part of the drawing. */
export function Mark(props: { size?: number }) {
  return (
    <svg
      width={props.size ?? 18}
      height={props.size ?? 18}
      viewBox="0 0 24 24"
      fill="none"
      stroke="#EADDC5"
      stroke-width="1.5"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      innerHTML={
        '<path d="M9 4.2h11v15.6H9z"/><path d="M2.4 6.6 6.4 12"/><path d="M2.4 12h4"/>' +
        '<path d="M2.4 17.4 6.4 12"/><path d="M6.4 12H9"/>' +
        '<path d="M12.6 9.2h4.2v5.6h-4.2z" fill="#f5901e" stroke="none"/>'
      }
    />
  );
}
