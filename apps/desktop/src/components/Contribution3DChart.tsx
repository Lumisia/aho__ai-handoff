import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Canvas } from "@react-three/fiber";
import { OrbitControls, OrthographicCamera } from "@react-three/drei";
import * as THREE from "three";
import type { Translator } from "../i18n";
import { formatCost, formatTokens } from "./TokenUsageChart";

export type Agent = "codex" | "claude" | "other";

export interface IsoSeg {
  key: string;
  agent: Agent;
  value: number;
}

export interface IsoCell {
  col: number;
  row: number;
  label: string;
  total: number;
  cost: number;
  events: number;
  /** Bottom-first: index 0 sits on the floor. Biggest agent first. */
  segments: IsoSeg[];
}

interface Props {
  cells: IsoCell[];
  cols: number;
  rows: number;
  maxTokens: number;
  t: Translator;
}

const CELL = 1;
const GAP = 0.15;
const STEP = CELL + GAP;
const BASE_HEIGHT = 0.05; // flat floor tiles for days with no usage
const MAX_HEIGHT = 4.0;

const AGENT_LABEL: Record<Agent, string> = {
  codex: "GPT/Codex",
  claude: "Claude",
  other: "Other",
};

interface ThemeColors {
  codex: THREE.Color;
  claude: THREE.Color;
  other: THREE.Color;
  floorTop: THREE.Color;
  floorSide: THREE.Color;
  accent: string;
}

function readColors(): ThemeColors {
  const cs = getComputedStyle(document.documentElement);
  const hex = (name: string, fallback: string) => {
    const v = cs.getPropertyValue(name).trim();
    return v && v.startsWith("#") ? v : fallback;
  };
  const panel = new THREE.Color(hex("--panel-bg-color", "#ffffff"));
  const ink = new THREE.Color(hex("--text-color", "#17201c"));
  return {
    codex: new THREE.Color(hex("--token-codex-fixed", "#b99deb")),
    claude: new THREE.Color(hex("--token-claude-fixed", "#dc9448")),
    other: new THREE.Color(hex("--focus-border-color", "#2f6f50")),
    floorTop: panel.clone().lerp(ink, 0.05),
    floorSide: panel.clone().lerp(ink, 0.12),
    accent: hex("--focus-border-color", "#2563eb"),
  };
}

interface HoverInfo {
  cell: IsoCell;
  x: number;
  y: number;
}

export default function Contribution3DChart({ cells, cols, rows, maxTokens, t }: Props) {
  const [hover, setHover] = useState<HoverInfo | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const controlsRef = useRef<any | null>(null);
  const invalidateRef = useRef<(() => void) | null>(null);

  const colors = useMemo(readColors, []);
  const totalWidth = cols * STEP;
  const totalDepth = rows * STEP;
  const offsetX = -totalWidth / 2;
  const offsetZ = -totalDepth / 2;
  const max = Math.max(maxTokens, 1);

  const colorFor = (agent: Agent) =>
    agent === "codex" ? colors.codex : agent === "claude" ? colors.claude : colors.other;

  const placed = useMemo(() => {
    return cells.map((cell) => {
      const x = offsetX + cell.col * STEP + STEP / 2;
      const z = offsetZ + cell.row * STEP + STEP / 2;
      const active = cell.total > 0;
      const height = active
        ? BASE_HEIGHT + Math.pow(cell.total / max, 0.6) * MAX_HEIGHT
        : BASE_HEIGHT;
      return { cell, x, z, active, height };
    });
  }, [cells, offsetX, offsetZ, max]);

  const initialCam = useMemo(
    () => ({
      px: totalWidth * 0.7,
      py: totalWidth * 0.45,
      pz: totalWidth * 0.7,
      zoom: 10,
    }),
    [totalWidth],
  );

  function fitView() {
    const ctrl = controlsRef.current;
    const wrap = wrapRef.current;
    if (!ctrl || !wrap) return;
    const cam = ctrl.object as THREE.OrthographicCamera;
    ctrl.target.set(0, 0, 0);
    cam.position.set(totalWidth * 0.7, totalWidth * 0.45, totalWidth * 0.7);
    cam.up.set(0, 1, 0);
    cam.lookAt(0, 0, 0);
    cam.updateMatrixWorld(true);
    const w = wrap.clientWidth, h = wrap.clientHeight;
    if (!w || !h) { ctrl.update(); return; }

    const corners: THREE.Vector3[] = [];
    const active = placed.filter((p) => p.active);
    const half = CELL / 2;
    if (active.length > 0) {
      for (const p of active) {
        for (const dx of [-half, half]) {
          for (const dz of [-half, half]) {
            for (const sy of [0, p.height]) {
              corners.push(new THREE.Vector3(p.x + dx, sy, p.z + dz));
            }
          }
        }
      }
    } else {
      const hx = totalWidth / 2, hz = totalDepth / 2;
      for (const sx of [-hx, hx]) {
        for (const sz of [-hz, hz]) {
          for (const sy of [0, MAX_HEIGHT]) {
            corners.push(new THREE.Vector3(sx, sy, sz));
          }
        }
      }
    }
    const inv = new THREE.Matrix4().copy(cam.matrixWorld).invert();
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for (const c of corners) {
      const v = c.clone().applyMatrix4(inv);
      if (v.x < minX) minX = v.x;
      if (v.x > maxX) maxX = v.x;
      if (v.y < minY) minY = v.y;
      if (v.y > maxY) maxY = v.y;
    }
    const sw = Math.max(maxX - minX, 0.0001);
    const sh = Math.max(maxY - minY, 0.0001);
    const padding = 0.85;
    cam.zoom = Math.min((w * padding) / sw, (h * padding) / sh);
    const cx = (minX + maxX) / 2;
    const cy = (minY + maxY) / 2;
    if (Math.abs(cx) > 0.001 || Math.abs(cy) > 0.001) {
      const right = new THREE.Vector3(1, 0, 0).applyQuaternion(cam.quaternion);
      const upv = new THREE.Vector3(0, 1, 0).applyQuaternion(cam.quaternion);
      const offset = new THREE.Vector3().addScaledVector(right, cx).addScaledVector(upv, cy);
      ctrl.target.add(offset);
      cam.position.add(offset);
    }
    cam.updateProjectionMatrix();
    ctrl.update();
    invalidateRef.current?.();
  }

  // Auto-fit on mount, whenever the dataset changes (period/breakdown switch —
  // keyed on dimensions + magnitude so switching e.g. model↔source refits even
  // at the same grid size), and on resize. Manual orbit/pan/zoom persists until
  // the next data change.
  //
  // On first mount the OrbitControls ref and the canvas size are often not ready
  // on the first frame, so a single rAF would silently skip the fit and leave
  // the chart zoomed out until the user pressed "Fit". Retry across frames until
  // both are ready.
  useEffect(() => {
    const wrap = wrapRef.current;
    if (!wrap) return;
    let raf = 0;
    let tries = 0;
    const ready = () => !!(wrap.clientWidth && wrap.clientHeight && controlsRef.current);
    const tryFit = () => {
      if (ready()) {
        fitView();
        return;
      }
      if (tries++ < 90) raf = requestAnimationFrame(tryFit);
    };
    raf = requestAnimationFrame(tryFit);
    const ro = new ResizeObserver(() => {
      if (ready()) fitView();
    });
    ro.observe(wrap);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cols, rows, cells.length, maxTokens]);

  return (
    <div className="token-iso" ref={wrapRef}>
      <Canvas
        dpr={[1, 2]}
        frameloop="demand"
        gl={{ antialias: true, alpha: true }}
        style={{ width: "100%", height: "100%" }}
        onCreated={(state) => {
          invalidateRef.current = state.invalidate;
        }}
      >
        <OrthographicCamera
          makeDefault
          position={[initialCam.px, initialCam.py, initialCam.pz]}
          zoom={initialCam.zoom}
          near={-2000}
          far={2000}
        />
        <OrbitControls
          ref={controlsRef as any}
          target={[0, 0, 0]}
          enableRotate
          enablePan
          enableZoom
          zoomToCursor
          panSpeed={1.0}
          rotateSpeed={0.7}
          minZoom={1}
          maxZoom={120}
          onChange={() => invalidateRef.current?.()}
        />
        <ambientLight intensity={0.78} />
        <directionalLight position={[20, 30, 15]} intensity={0.8} />
        <directionalLight position={[-15, 20, -10]} intensity={0.25} />

        {placed.map(({ cell, x, z, active, height }) => {
          if (!active) {
            return (
              <mesh key={`${cell.col}-${cell.row}`} position={[x, BASE_HEIGHT / 2, z]}>
                <boxGeometry args={[CELL, BASE_HEIGHT, CELL]} />
                <meshStandardMaterial color={colors.floorTop} roughness={0.9} metalness={0} />
              </mesh>
            );
          }
          const setFrom = (e: any) => {
            e.stopPropagation();
            setHover({ cell, x: e.clientX, y: e.clientY });
          };
          let y = 0;
          return (
            <group
              key={`${cell.col}-${cell.row}`}
              onPointerOver={setFrom}
              onPointerMove={(e: any) => setHover((h) => (h ? { ...h, x: e.clientX, y: e.clientY } : { cell, x: e.clientX, y: e.clientY }))}
              onPointerOut={() => setHover(null)}
            >
              {cell.segments
                .filter((s) => s.value > 0)
                .map((seg) => {
                  const segH = Math.max((seg.value / cell.total) * height, 0.001);
                  const cy = y + segH / 2;
                  y += segH;
                  return (
                    <mesh key={seg.key} position={[x, cy, z]}>
                      <boxGeometry args={[CELL, segH, CELL]} />
                      <meshStandardMaterial color={colorFor(seg.agent)} roughness={0.55} metalness={0.04} />
                    </mesh>
                  );
                })}
            </group>
          );
        })}
      </Canvas>

      {hover &&
        createPortal(
          <div className="token-iso-tip" style={{ left: hover.x + 14, top: hover.y + 14 }}>
            <strong>{hover.cell.label}</strong>
            <span className="token-iso-tip-total">
              {formatTokens(hover.cell.total)} · {formatCost(hover.cell.cost)}
            </span>
            {hover.cell.segments
              .filter((s) => s.value > 0)
              .map((s) => (
                <span className="token-iso-tip-row" key={s.key}>
                  <i className={s.agent} />
                  {AGENT_LABEL[s.agent]}
                  <b>{formatTokens(s.value)}</b>
                </span>
              ))}
            <span className="token-iso-tip-events">
              {hover.cell.events} {t("events")}
            </span>
          </div>,
          document.body,
        )}

      <div className="token-iso-actions">
        <button className="token-iso-btn primary" style={{ borderColor: colors.accent, color: colors.accent }} onClick={fitView}>
          Fit
        </button>
        <button className="token-iso-btn" onClick={fitView}>
          Reset
        </button>
      </div>
    </div>
  );
}
