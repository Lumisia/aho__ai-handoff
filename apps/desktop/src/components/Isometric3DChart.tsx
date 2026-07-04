import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Translator } from "../i18n";
import { formatCost, formatTokens } from "./TokenUsageChart";

export interface IsoSegment {
  key: string;
  value: number;
  agent: "codex" | "claude" | "other";
}

export interface IsoColumn {
  id: string;
  label: string;
  total: number;
  cost: number;
  events: number;
  /** Bottom-first: index 0 sits on the floor, later segments stack on top. */
  segments: IsoSegment[];
}

interface Point {
  x: number;
  y: number;
}

interface HitRegion {
  column: IsoColumn;
  poly: Point[];
}

// Isometric basis in CSS pixels. Width marches right, depth recedes back-up-right,
// height rises straight up. Small depth keeps the strip readable like the reference.
const CUBE_W = 15; // front face width
const DEPTH = { x: 8, y: -5 }; // back-up-right
const COL_GAP = 7; // horizontal gap between columns
const COL_DRIFT = 3; // per-column upward drift so the row reads as a 3D strip
const MAX_BAR_PX = 128; // tallest stack in pixels
const FLOOR_PAD = 14;
const MIN_SEG_PX = 4;

const AGENT_VAR: Record<IsoSegment["agent"], string> = {
  codex: "--token-codex-fixed",
  claude: "--token-claude-fixed",
  other: "--focus-border-color",
};

const AGENT_FALLBACK: Record<IsoSegment["agent"], string> = {
  codex: "#b99deb",
  claude: "#dc9448",
  other: "#2f6f50",
};

const AGENT_LABEL: Record<IsoSegment["agent"], string> = {
  codex: "GPT/Codex",
  claude: "Claude",
  other: "Other",
};

function add(a: Point, b: Point): Point {
  return { x: a.x + b.x, y: a.y + b.y };
}

function up(base: Point, h: number): Point {
  return { x: base.x, y: base.y - h };
}

function pointInPoly(pt: Point, poly: Point[]): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const a = poly[i];
    const b = poly[j];
    const intersect =
      a.y > pt.y !== b.y > pt.y &&
      pt.x < ((b.x - a.x) * (pt.y - a.y)) / (b.y - a.y) + a.x;
    if (intersect) inside = !inside;
  }
  return inside;
}

export default function Isometric3DChart({
  columns,
  t,
  ariaLabel,
}: {
  columns: IsoColumn[];
  t: Translator;
  ariaLabel: string;
}) {
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const hitsRef = useRef<HitRegion[]>([]);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [hover, setHover] = useState<{ column: IsoColumn; x: number; y: number } | null>(null);

  useLayoutEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const measure = () => setSize({ w: el.clientWidth, h: el.clientHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || size.w === 0 || size.h === 0) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(size.w * dpr);
    canvas.height = Math.round(size.h * dpr);
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, size.w, size.h);

    const cs = getComputedStyle(canvas);
    const cssVar = (name: string, fallback: string) =>
      cs.getPropertyValue(name).trim() || fallback;
    const colorFor = (agent: IsoSegment["agent"]) =>
      cssVar(AGENT_VAR[agent], AGENT_FALLBACK[agent]);
    const gridInk = cssVar("--text-color", "#888888");

    const n = columns.length;
    const maxTotal = Math.max(1, ...columns.map((c) => c.total));

    // Layout: columns march right with a gentle upward drift, centered, floor near
    // the bottom. Geometry scales down (factor k) so every column fits the width.
    const availW = size.w - FLOOR_PAD * 2;
    const natTail = CUBE_W + DEPTH.x;
    const natStep = CUBE_W + DEPTH.x + COL_GAP;
    const natStrip = n > 1 ? (n - 1) * natStep + natTail : natTail;
    const k = natStrip > availW ? Math.max(0.35, availW / natStrip) : 1;

    const cubeW = CUBE_W * k;
    const depth: Point = { x: DEPTH.x * k, y: DEPTH.y * k };
    const drift = COL_DRIFT * k;
    const stepX = natStep * k;
    const stripW = natStrip * k;
    const originX = Math.max(FLOOR_PAD, (size.w - stripW) / 2);
    const floorY = size.h - FLOOR_PAD - 6;
    // Cap bar height so the tallest stack plus the upward drift never clips the top.
    const maxBar = Math.min(MAX_BAR_PX, size.h - FLOOR_PAD - 6 - (n - 1) * drift - 12);

    const baseFor = (i: number): Point => ({
      x: originX + i * stepX,
      y: floorY - i * drift,
    });

    // --- Floor grid ---
    if (n > 0) {
      const first = baseFor(0);
      const last = baseFor(n - 1);
      const fl = { x: first.x - FLOOR_PAD, y: first.y + 6 };
      const fr = { x: last.x + cubeW + FLOOR_PAD, y: last.y + 6 };
      const backFl = add(fl, depth);
      const backFr = add(fr, depth);
      ctx.beginPath();
      ctx.moveTo(fl.x, fl.y);
      ctx.lineTo(fr.x, fr.y);
      ctx.lineTo(backFr.x, backFr.y);
      ctx.lineTo(backFl.x, backFl.y);
      ctx.closePath();
      ctx.fillStyle = gridInk;
      ctx.globalAlpha = 0.05;
      ctx.fill();
      ctx.globalAlpha = 0.13;
      ctx.lineWidth = 1;
      ctx.strokeStyle = gridInk;
      // depth-wise lines (front edge -> back edge) every column step
      const cells = n + 1;
      for (let i = 0; i <= cells; i++) {
        const tt = i / cells;
        const fx = fl.x + (fr.x - fl.x) * tt;
        const fy = fl.y + (fr.y - fl.y) * tt;
        ctx.beginPath();
        ctx.moveTo(fx, fy);
        ctx.lineTo(fx + depth.x, fy + depth.y);
        ctx.stroke();
      }
      // one back rail
      ctx.beginPath();
      ctx.moveTo(backFl.x, backFl.y);
      ctx.lineTo(backFr.x, backFr.y);
      ctx.stroke();
      ctx.globalAlpha = 1;
    }

    // --- Cubes (painter's order: far/back columns first) ---
    const hits: HitRegion[] = [];
    for (let i = n - 1; i >= 0; i--) {
      const col = columns[i];
      const base = baseFor(i);
      const fl = base; // front-left-bottom
      const fr = add(base, { x: cubeW, y: 0 }); // front-right-bottom
      const totalPx =
        col.total > 0 ? Math.max(MIN_SEG_PX, (col.total / maxTotal) * maxBar) : 0;

      // silhouette for hit-testing (front-left-bottom around to top)
      const flT = up(fl, totalPx);
      const frT = up(fr, totalPx);
      const brB = add(fr, depth);
      const brT = up(brB, totalPx);
      const blT = up(add(fl, depth), totalPx);
      hits.push({
        column: col,
        poly: [fl, fr, brB, brT, blT, flT],
      });

      if (totalPx <= 0) continue;

      const isHover = hover?.column.id === col.id;
      let hb = 0;
      for (const seg of col.segments) {
        const share = col.total > 0 ? seg.value / col.total : 0;
        const segPx = Math.max(MIN_SEG_PX * 0.6, share * totalPx);
        const ht = hb + segPx;
        const baseColor = colorFor(seg.agent);
        drawCube(ctx, fl, fr, hb, ht, baseColor, isHover, depth);
        hb = ht;
      }
    }
    hitsRef.current = hits;
  }, [columns, size, hover]);

  function onMove(ev: React.MouseEvent<HTMLDivElement>) {
    const wrap = wrapRef.current;
    if (!wrap) return;
    const rect = wrap.getBoundingClientRect();
    const pt = { x: ev.clientX - rect.left, y: ev.clientY - rect.top };
    // nearest-first: hits stored back-to-front, so search reverse for the front cube
    for (let i = hitsRef.current.length - 1; i >= 0; i--) {
      const h = hitsRef.current[i];
      if (pointInPoly(pt, h.poly)) {
        setHover({ column: h.column, x: pt.x, y: pt.y });
        return;
      }
    }
    setHover(null);
  }

  function onLeave() {
    setHover(null);
  }

  const tipW = 168;
  const tipLeft = hover
    ? Math.min(Math.max(8, hover.x + 12), Math.max(8, size.w - tipW - 8))
    : 0;

  return (
    <div
      ref={wrapRef}
      className="token-iso"
      aria-label={ariaLabel}
      onMouseMove={onMove}
      onMouseLeave={onLeave}
    >
      <canvas ref={canvasRef} className="token-iso-canvas" style={{ width: "100%", height: "100%" }} />
      {hover && (
        <div
          className="token-iso-tip"
          style={{ left: tipLeft, top: Math.max(6, hover.y - 12), width: tipW }}
        >
          <strong>{hover.column.label}</strong>
          <span className="token-iso-tip-total">
            {formatTokens(hover.column.total)} · {formatCost(hover.column.cost)}
          </span>
          {hover.column.segments
            .filter((s) => s.value > 0)
            .map((s) => (
              <span className="token-iso-tip-row" key={s.key}>
                <i className={s.agent} />
                {AGENT_LABEL[s.agent]}
                <b>{formatTokens(s.value)}</b>
              </span>
            ))}
          <span className="token-iso-tip-events">
            {hover.column.events} {t("events")}
          </span>
        </div>
      )}
    </div>
  );
}

function drawCube(
  ctx: CanvasRenderingContext2D,
  fl: Point,
  fr: Point,
  hb: number,
  ht: number,
  base: string,
  highlight: boolean,
  depth: Point,
) {
  const flB = up(fl, hb);
  const frB = up(fr, hb);
  const flT = up(fl, ht);
  const frT = up(fr, ht);
  const brB = up(add(fr, depth), hb);
  const brT = up(add(fr, depth), ht);
  const blT = up(add(fl, depth), ht);

  // front face (base tone)
  fillFace(ctx, [flB, frB, frT, flT], base, highlight ? 0.14 : 0, "#ffffff");
  // right/depth face (darker)
  fillFace(ctx, [frB, brB, brT, frT], base, 0.28, "#000000");
  // top face (lighter)
  fillFace(ctx, [flT, frT, brT, blT], base, highlight ? 0.36 : 0.22, "#ffffff");
}

function fillFace(
  ctx: CanvasRenderingContext2D,
  poly: Point[],
  base: string,
  overlay: number,
  overlayColor: string,
) {
  ctx.beginPath();
  ctx.moveTo(poly[0].x, poly[0].y);
  for (let i = 1; i < poly.length; i++) ctx.lineTo(poly[i].x, poly[i].y);
  ctx.closePath();
  ctx.fillStyle = base;
  ctx.fill();
  if (overlay > 0) {
    ctx.fillStyle =
      overlayColor === "#000000"
        ? `rgba(0,0,0,${overlay})`
        : `rgba(255,255,255,${overlay})`;
    ctx.fill();
  }
  ctx.lineWidth = 1;
  ctx.strokeStyle = "rgba(0,0,0,0.22)";
  ctx.stroke();
}
