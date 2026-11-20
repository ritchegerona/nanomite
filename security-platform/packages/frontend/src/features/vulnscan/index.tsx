import { useState } from 'react';
import { Play } from 'lucide-react';

export function VulnScanSpace() {
  const [target, setTarget] = useState('');
  const [scanner, setScanner] = useState('nuclei');

  const scanners = [
    { id: 'nuclei', name: 'Nuclei', description: 'Template-based vuln scan' },
    { id: 'openvas', name: 'OpenVAS', description: 'Network vulnerability assessment' },
    { id: 'trivy', name: 'Trivy', description: 'Container image scanner' },
    { id: 'checkov', name: 'Checkov', description: 'IaC security scanner' },
    { id: 'zap', name: 'OWASP ZAP', description: 'Web app security scanner' },
  ];

  return (
    <div className="space-y-6">
      <div className="flex flex-col md:flex-row md:items-center md:justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold">Vulnerability Scanning</h1>
          <p className="text-muted-foreground">Blue Team vulnerability assessment across network, cloud, container, and code</p>
        </div>
        <button className="px-4 py-2 bg-primary text-white rounded-lg hover:bg-primary/90 transition-colors text-sm font-medium flex items-center gap-2">
          <Play className="h-4 w-4" />
          New Scan
        </button>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-4 gap-4">
        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">Scan Configuration</h3>
          <div className="space-y-3 mt-2">
            <label className="text-xs font-semibold">Target</label>
            <input
              value={target}
              onChange={(e) => setTarget(e.target.value)}
              placeholder="10.0.0.0/16"
              className="w-full px-3 py-2 border rounded-lg focus:ring-2 focus:ring-primary outline-none"
            />
            <label className="text-xs font-semibold">Scanner</label>
            <select
              value={scanner}
              onChange={(e) => setScanner(e.target.value)}
              className="w-full px-3 py-2 border rounded-lg focus:ring-2 focus:ring-primary outline-none bg-background"
            >
              {scanners.map((s) => (
                <option key={s.id} value={s.id}>{s.name}</option>
              ))}
            </select>
          </div>
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">Critical</h3>
          <div className="text-3xl font-bold text-red-500">3</div>
          <p className="text-xs text-muted-foreground">Immediate action required</p>
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">High</h3>
          <div className="text-3xl font-bold text-orange-500">7</div>
          <p className="text-xs text-muted-foreground">Patch within 7 days</p>
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">Medium</h3>
          <div className="text-3xl font-bold text-yellow-500">14</div>
          <p className="text-xs text-muted-foreground">Patch within 30 days</p>
        </div>
      </div>

      <div className="lg:col-span-4">
        <div className="border rounded-lg bg-card p-6">
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-lg font-semibold">Findings</h2>
            <a href="#" className="text-sm text-primary hover:underline">Export to Jira</a>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead className="bg-secondary/50">
                <tr>
                  <th className="px-3 py-2 text-left">Severity</th>
                  <th className="px-3 py-2 text-left">Title</th>
                  <th className="px-3 py-2 text-left">Target</th>
                  <th className="px-3 py-2 text-left">CVSS</th>
                  <th className="px-3 py-2 text-left">Status</th>
                </tr>
              </thead>
              <tbody>
                {[
                  { sev: 'Critical', title: 'SQL Injection in Login Form', target: 'web-01.example.com', cvss: 9.8, status: 'Open' },
                  { sev: 'High', title: 'Outdated Apache Version', target: 'web-02.example.com', cvss: 7.5, status: 'Open' },
                  { sev: 'Medium', title: 'Missing Security Headers', target: 'api.example.com', cvss: 5.3, status: 'Triaged' },
                ].map((f, i) => (
                  <tr key={i} className="border-b">
                    <td className="px-3 py-2">
                      <span className={f.sev === 'Critical' ? 'px-2 py-0.5 rounded text-xs font-bold bg-red-500/10 text-red-500' : f.sev === 'High' ? 'px-2 py-0.5 rounded text-xs font-bold bg-orange-500/10 text-orange-500' : 'px-2 py-0.5 rounded text-xs font-bold bg-yellow-500/10 text-yellow-500'}>
                        {f.sev}
                      </span>
                    </td>
                    <td className="px-3 py-2 font-medium">{f.title}</td>
                    <td className="px-3 py-2 font-mono">{f.target}</td>
                    <td className="px-3 py-2">{f.cvss}</td>
                    <td className="px-3 py-2">
                      <span className={f.status === 'Open' ? 'px-2 py-0.5 rounded text-xs bg-red-500/10 text-red-500' : 'px-2 py-0.5 rounded text-xs bg-blue-500/10 text-blue-500'}>
                        {f.status}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  );
}