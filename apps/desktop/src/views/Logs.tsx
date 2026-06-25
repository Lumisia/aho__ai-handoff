import { useEffect, useState } from "react";
import { readLogs } from "../api";
import type { LogFile } from "../types";

export default function Logs() {
  const [logs, setLogs] = useState<LogFile[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    readLogs().then(setLogs).catch((err) => {
      setError(err instanceof Error ? err.message : String(err));
    });
  }, []);

  if (error) {
    return <section className="banner error">Failed to read logs: {error}</section>;
  }

  return (
    <div className="view list">
      {logs.map((log) => (
        <article className="row" key={log.name}>
          <strong>{log.name}</strong>
          {log.result.error && <p>{log.result.error}</p>}
          {!log.result.error && <pre>{log.result.text || "Empty log."}</pre>}
        </article>
      ))}
    </div>
  );
}
