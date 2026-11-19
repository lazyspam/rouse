import {
  siAdonisjs,
  siAngular,
  siAstro,
  siBun,
  siDeno,
  siDjango,
  siDocker,
  siElectron,
  siExpo,
  siExpress,
  siFastapi,
  siFastify,
  siFlask,
  siGatsby,
  siGo,
  siHono,
  siKoa,
  siLaravel,
  siNestjs,
  siNextdotjs,
  siNodedotjs,
  siNuxt,
  siPhp,
  siPython,
  siQwik,
  siReact,
  siReactrouter,
  siRemix,
  siRuby,
  siRubyonrails,
  siRust,
  siSolid,
  siStrapi,
  siSvelte,
  siTauri,
  siTypescript,
  siVite,
  siVuedotjs,
  type SimpleIcon,
} from "simple-icons";
import type { Logo } from "../services/tauri";

/** Every slug the backend's stack detection (icons.rs) can return. */
const TECH: Record<string, SimpleIcon> = Object.fromEntries(
  [
    siAdonisjs, siAngular, siAstro, siBun, siDeno, siDjango, siDocker, siElectron, siExpo, siExpress, siFastapi,
    siFastify, siFlask, siGatsby, siGo, siHono, siKoa, siLaravel, siNestjs, siNextdotjs, siNodedotjs, siNuxt, siPhp,
    siPython, siQwik, siReact, siReactrouter, siRemix, siRuby, siRubyonrails, siRust, siSolid, siStrapi, siSvelte,
    siTauri, siTypescript, siVite, siVuedotjs,
  ].map((icon) => [icon.slug, icon]),
);

/** Brand colours too dark for the dark background (Express, Next, Rust are black) draw in the text colour. */
function visible(hex: string) {
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  return lum < 0.25 ? "var(--color-fg)" : `#${hex}`;
}

/** The project's own logo, else its stack's (NestJS, Express, Rust…), else a quiet gap. */
export function ProjectLogo({ logo, size = 14 }: { logo?: Logo; size?: number }) {
  if (logo?.kind === "image") {
    return <img src={logo.src} alt="" width={size} height={size} className="shrink-0 rounded-[3px] object-contain" />;
  }
  const icon = logo?.kind === "tech" ? TECH[logo.slug] : undefined;
  if (!icon) return <span className="shrink-0" style={{ width: size }} />;
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} className="shrink-0" fill={visible(icon.hex)} role="img" aria-label={icon.title}>
      <path d={icon.path} />
    </svg>
  );
}
