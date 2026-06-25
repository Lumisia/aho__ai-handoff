import type { DashboardSnapshot } from "../types";

export default function Doctor({ snapshot }: { snapshot: DashboardSnapshot }) {
  return (
    <div className="view list">
      {snapshot.checks.map((check) => (
        <article className={`row ${check.status}`} key={check.id}>
          <strong>{check.label}</strong>
          <span>{check.status}</span>
          <p>{check.message}</p>
          {check.path && <code>{check.path}</code>}
        </article>
      ))}
    </div>
  );
}
