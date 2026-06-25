import type { CheckRow, DashboardSnapshot } from "../types";

function StatusCard({ row }: { row: CheckRow }) {
  return (
    <article className={`card ${row.status}`}>
      <div className="card-head">
        <span>{row.label}</span>
        <strong>{row.status}</strong>
      </div>
      <p>{row.message}</p>
      {row.path && <code>{row.path}</code>}
    </article>
  );
}

export default function Overview({ snapshot }: { snapshot: DashboardSnapshot }) {
  const topRows = [
    snapshot.daemon,
    snapshot.autostart,
    snapshot.codex_hooks,
    snapshot.codex_config,
    snapshot.claude_settings,
    snapshot.ipc,
    snapshot.store,
  ];

  return (
    <div className="view">
      <section className="metrics">
        <div>
          <span>Pending</span>
          <strong>{snapshot.capsules.pending_count}</strong>
        </div>
        <div>
          <span>Total capsules</span>
          <strong>{snapshot.capsules.items.length}</strong>
        </div>
        <div>
          <span>Skipped files</span>
          <strong>{snapshot.capsules.skipped}</strong>
        </div>
        <div>
          <span>Autostart</span>
          <strong>{snapshot.install_state.autostart}</strong>
        </div>
      </section>
      <section className="grid">
        {topRows.map((row) => (
          <StatusCard key={row.id} row={row} />
        ))}
      </section>
    </div>
  );
}
