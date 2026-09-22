import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import { useApp } from "~/state/store";

/**
 * The one screen that stands as a placeholder in this version, by the person's
 * decision. It says exactly what it will hold and what to do until then.
 */
export function DownloadScreen() {
  const app = useApp();
  return (
    <>
      <Header title="Download" sub="not in this version" />
      <div class="screen">
        <div class="empty">
          <span class="gl">
            <Icon name="download" size={30} />
          </span>
          <h2>Downloads land here later</h2>
          <p>
            A model pulled from Hugging Face or Civitai will be written straight
            into the vault, checked against what is already there, and linked into
            the installs you choose. None of that is built yet.
          </p>
          <div style={{ display: "flex", gap: "8px", "margin-top": "2px" }}>
            <button class="btn" disabled>
              Hugging Face
            </button>
            <button class="btn" disabled>
              Civitai
            </button>
          </div>
          <div class="foot">
            Destination &nbsp;
            <span class="emph">{app.vault()?.root ?? "C:\\ComfyVault"}</span>
            <br />
            Until then, download where you always do and run a scan.
          </div>
        </div>
      </div>
    </>
  );
}
