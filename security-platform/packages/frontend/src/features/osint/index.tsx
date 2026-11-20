import { useState } from 'react';
import { Globe } from 'lucide-react';

export function OSINTSpace() {
  const [target, setTarget] = useState('');
  const [results, setResults] = useState<any[]>([]);
  const [loading, setLoading] = useState(false);

  const handleScan = async () => {
    if (!target) return;
    setLoading(true);
    setResults([
      { subdomain: `www.${target}`, ip: '1.2.3.4', status: 'live' },
      { subdomain: `api.${target}`, ip: '5.6.7.8', status: 'live' },
      { subdomain: `dev.${target}`, ip: '9.10.11.12', status: 'dead' },
    ]);
    setLoading(false);
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-col md:flex-row md:items-center md:justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold">OSINT Reconnaissance</h1>
          <p className="text-muted-foreground">Open source intelligence gathering for target infrastructure</p>
        </div>
        <button className="px-4 py-2 bg-primary text-white rounded-lg hover:bg-primary/90 transition-colors text-sm font-medium flex items-center gap-2">
          <Globe className="h-4 w-4" />
          New Recon
        </button>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-4 gap-4">
        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">Target</h3>
          <input
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            placeholder="example.com"
            className="w-full mt-2 px-3 py-2 border rounded-lg focus:ring-2 focus:ring-primary outline-none"
          />
          <button
            onClick={handleScan}
            disabled={loading || !target}
            className="w-full mt-3 py-2 bg-primary text-white rounded-lg hover:bg-primary/90 disabled:opacity-50 transition-colors"
          >
            {loading ? 'Scanning...' : 'Start Recon'}
          </button>
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">Subdomains</h3>
          <div className="space-y-2">
            {results.map((r, i) => (
              <div key={i} className="flex items-center justify-between text-sm p-2 rounded bg-secondary/50">
                <span className="font-mono">{r.subdomain}</span>
                <span className={(['text-xs px-2 py-0.5 rounded', r.status === 'live' ? 'bg-green-500/10 text-green-500' : 'bg-red-500/10 text-red-500']).join(' ')}>
                  {r.status}
                </span>
              </div>
            ))}
          </div>
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">Open Ports</h3>
          <div className="space-y-2">
            {[22, 80, 443, 8080].map((port) => (
              <div key={port} className="flex items-center justify-between text-sm p-2 rounded bg-secondary/50">
                <span className="font-mono">{port}/tcp</span>
                <span className="text-xs text-muted-foreground">open</span>
              </div>
            ))}
          </div>
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">Technologies</h3>
          <div className="space-y-2">
            {['Nginx', 'PHP 7.4', 'Cloudflare', 'jQuery'].map((tech, i) => (
              <div key={i} className="flex items-center justify-between text-sm p-2 rounded bg-secondary/50">
                <span>{tech}</span>
                <span className="text-xs text-muted-foreground">detected</span>
              </div>
            ))}
          </div>
        </div>
      </div>

      <div className="lg:col-span-4">
        <div className="border rounded-lg bg-card p-6">
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-lg font-semibold">Tool Output</h2>
            <button className="text-sm text-primary hover:underline">Refresh</button>
          </div>
          <div className="h-64 overflow-auto bg-black/50 rounded-lg p-3 border border-border font-mono text-xs text-green-400">
            {loading
              ? 'Running subdomain enumeration...\n[+] Querying crt.sh...\n[+] Running amass...\n[+] Running subfinder...\n'
              : 'Ready to run reconnaissance tools.\n\nAvailable tools:\n- subdomain_enum\n- port_scan\n- dns_enum\n- tech_fingerprint\n- ssl_tls\n- whois\n- cloud_enum\n- git_recon'}
          </div>
        </div>
      </div>
    </div>
  );
}