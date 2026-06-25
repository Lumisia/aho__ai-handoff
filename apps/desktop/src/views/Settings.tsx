import type { DashboardSnapshot } from "../types";

export default function SettingsView({ snapshot }: { snapshot: DashboardSnapshot }) {
  return (
    <div className="view list">
      {Object.entries(snapshot.paths).map(([key, value]) => (
        <article className="row" key={key}>
          <strong>{key}</strong>
          <code>{value}</code>
        </article>
      ))}
      <article className="row">
        <strong>Install state</strong>
        <p>version {snapshot.install_state.version}</p>
        <p>{snapshot.install_state.installed_at || "not installed"}</p>
        {snapshot.install_state.launcher && <code>{snapshot.install_state.launcher}</code>}
      </article>
    </div>
  );
}
