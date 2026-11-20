import { useState } from 'react';
import { Play } from 'lucide-react';

export function SIEMKeySpace() {
  const [query, setQuery] = useState('');

  return (
    <div className="space-y-6">
      <div className="flex flex-col md:flex-row md:items-center md:justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold">SIEM — Security Events</h1>
          <p className="text-muted-foreground">Blue Team log analysis, correlation, and threat hunting</p>
        </div>
        <div className="flex gap-2">
          <button className="px-4 py-2 bg-primary text-white rounded-lg hover:bg-primary/90 transition-colors text-sm font-medium flex items-center gap-2">
            <Play className="h-4 w-4" />
            Hunt
          </button>
          <button className="px-4 py-2 bg-secondary text-muted-foreground rounded-lg hover:bg-secondary/80 transition-colors text-sm font-medium">
            Export
          </button>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-4 gap-4">
        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">EPS Rate</h3>
          <div className="text-3xl font-bold">1,247</div>
          <p className="text-xs text-muted-foreground">events per second</p>
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">New Alerts</h3>
          <div className="text-3xl font-bold text-red-500">23</div>
          <p className="text-xs text-muted-foreground">Require triage</p>
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">MITRE Coverage</h3>
          <div className="text-3xl font-bold text-blue-500">78%</div>
          <p className="text-xs text-muted-foreground">ATT&CK techniques</p>
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">Log Sources</h3>
          <div className="text-3xl font-bold">12</div>
          <p className="text-xs text-muted-foreground">Active sources</p>
        </div>
      </div>

      <div className="lg:col-span-4">
        <div className="border rounded-lg bg-card p-6">
          <div className="flex items-center gap-3 mb-4">
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search logs... (e.g., index=firewall sourcetype=pan)"
              className="flex-1 px-3 py-2 border rounded-lg focus:ring-2 focus:ring-primary outline-none"
            />
            <button className="px-4 py-2 bg-secondary text-muted-foreground rounded-lg hover:bg-secondary/80 transition-colors text-sm">
              Search
            </button>
          </div>

          <div className="h-96 overflow-auto bg-black/50 rounded-lg p-3 border border-border font-mono text-xs">
            {[
              '2024-01-15 10:23:45 [INFO] Firewall: Connection allowed from 10.0.1.5 to 10.0.2.10:443',
              '2024-01-15 10:23:46 [ALERT] IDS: Possible SQL injection from 10.0.1.5',
              '2024-01-15 10:23:47 [INFO] Endpoint: Process powershell.exe started by user admin',
              '2024-01-15 10:23:48 [ALERT] SIEM: T1059.001 - PowerShell execution detected',
              '2024-01-15 10:23:49 [INFO] CloudTrail: CreateBucket event by user devops',
            ].map((line, i) => (
              <div key={i} className={`mb-1 ${line.includes('ALERT') ? 'text-red-400' : 'text-green-400'}`}>
                {line}
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}