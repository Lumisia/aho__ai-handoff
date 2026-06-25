import { Activity, ClipboardCheck, FolderKanban, ScrollText, Settings } from "lucide-react";
import { useEffect, useState } from "react";
import { getDashboardSnapshot } from "./api";
import type { DashboardSnapshot } from "./types";
import Capsules from "./views/Capsules";
import Doctor from "./views/Doctor";
import Logs from "./views/Logs";
import Overview from "./views/Overview";
import SettingsView from "./views/Settings";

type Tab = "overview" | "doctor" | "capsules" | "settings" | "logs";

const tabs: Array<{ id: Tab; label: string; icon: typeof Activity }> = [
  { id: "overview", label: "Overview", icon: Activity },
  { id: "doctor", label: "Doctor", icon: ClipboardCheck },
  { id: "capsules", label: "Capsules", icon: FolderKanban },
  { id: "settings", label: "Settings", icon: Settings },
  { id: "logs", label: "Logs", icon: ScrollText },
];

export default function App() {
  const [active, setActive] = useState<Tab>("overview");
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    try {
      setError(null);
      setSnapshot(await getDashboardSnapshot());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <div className="mark">AH</div>
          <div>
            <h1>AI Handoff</h1>
            <p>Local dashboard</p>
          </div>
        </div>
        <nav>
          {tabs.map((tab) => {
            const Icon = tab.icon;
            return (
              <button
                key={tab.id}
                className={active === tab.id ? "nav active" : "nav"}
                onClick={() => setActive(tab.id)}
                title={tab.label}
              >
                <Icon size={18} />
                <span>{tab.label}</span>
              </button>
            );
          })}
        </nav>
      </aside>
      <main>
        <header className="topbar">
          <div>
            <p className="eyebrow">Read-only MVP</p>
            <h2>{tabs.find((tab) => tab.id === active)?.label}</h2>
          </div>
          <button className="refresh" onClick={refresh}>
            Refresh
          </button>
        </header>
        {error && <section className="banner error">Failed to load dashboard: {error}</section>}
        {!snapshot && !error && <section className="empty">Loading local state...</section>}
        {snapshot && active === "overview" && <Overview snapshot={snapshot} />}
        {snapshot && active === "doctor" && <Doctor snapshot={snapshot} />}
        {snapshot && active === "capsules" && <Capsules initial={snapshot.capsules} />}
        {snapshot && active === "settings" && <SettingsView snapshot={snapshot} />}
        {snapshot && active === "logs" && <Logs />}
      </main>
    </div>
  );
}
