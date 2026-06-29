import { useState } from "react";
import { readCapsule } from "../api";
import type { CapsuleList, CapsuleSummary, ReadTextResult } from "../types";

export default function Capsules({ initial }: { initial: CapsuleList }) {
  const [selected, setSelected] = useState<CapsuleSummary | null>(initial.items[0] ?? null);
  const [raw, setRaw] = useState<ReadTextResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function select(item: CapsuleSummary) {
    setSelected(item);
    setError(null);
    try {
      setRaw(await readCapsule(item.path));
    } catch (err) {
      setRaw(null);
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <div className="split view">
      <section className="list">
        {initial.items.length === 0 && <div className="empty">No capsules found.</div>}
        {initial.items.map((item) => (
          <button className="capsule" key={item.path} onClick={() => void select(item)}>
            <strong>{item.summary_preview}</strong>
            <span>
              {`${item.source_agent} -> ${item.target_agent}`}
            </span>
            <small>
              {item.created_at} - {item.state}
            </small>
          </button>
        ))}
      </section>
      <section className="panel">
        {!selected && <div className="empty">Select a capsule.</div>}
        {selected && (
          <>
            <h3>{selected.capsule_id}</h3>
            <p>
              {selected.project_label}
              {selected.project_label !== selected.project_id && (
                <small> ({selected.project_id})</small>
              )}
            </p>
            <code>{selected.path}</code>
            {error && <p className="inline-error">{error}</p>}
            <pre>{raw?.text ?? "Select item to load raw JSON."}</pre>
          </>
        )}
      </section>
    </div>
  );
}
