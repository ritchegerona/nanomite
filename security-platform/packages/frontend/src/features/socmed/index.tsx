import { useState } from 'react';
import { Network } from 'lucide-react';

export function SocMedSpace() {
  const [target, setTarget] = useState('');
  const [targetType, setTargetType] = useState<'username' | 'email' | 'domain'>('username');

  return (
    <div className="space-y-6">
      <div className="flex flex-col md:flex-row md:items-center md:justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold">SocMed & Doxxing Intelligence</h1>
          <p className="text-muted-foreground">Social media profiling, identity correlation, and breach data</p>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-4 gap-4">
        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">Target</h3>
          <div className="space-y-2 mt-2">
            <select
              value={targetType}
              onChange={(e) => setTargetType(e.target.value as any)}
              className="w-full px-3 py-2 border rounded-lg focus:ring-2 focus:ring-primary outline-none bg-background"
            >
              <option value="username">Username</option>
              <option value="email">Email</option>
              <option value="domain">Domain</option>
            </select>
            <input
              value={target}
              onChange={(e) => setTarget(e.target.value)}
              placeholder="john_doe"
              className="w-full px-3 py-2 border rounded-lg focus:ring-2 focus:ring-primary outline-none"
            />
          </div>
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">Platforms Found</h3>
          <div className="space-y-2">
            {['LinkedIn', 'Twitter/X', 'GitHub', 'Instagram', 'Facebook'].map((p, i) => (
              <div key={i} className="flex items-center justify-between text-sm p-2 rounded bg-secondary/50">
                <span>{p}</span>
                <span className="text-xs text-green-500">● claimed</span>
              </div>
            ))}
          </div>
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">Breach Exposure</h3>
          <div className="text-3xl font-bold text-red-500">2</div>
          <p className="text-xs text-muted-foreground">data breaches found</p>
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h3 className="text-lg font-semibold">Risk Score</h3>
          <div className="text-3xl font-bold text-yellow-500">6.5/10</div>
          <p className="text-xs text-muted-foreground">Digital footprint exposure</p>
        </div>
      </div>

      <div className="lg:col-span-4">
        <div className="border rounded-lg bg-card p-6">
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-lg font-semibold">Identity Graph</h2>
            <span className="text-xs text-muted-foreground">Cross-platform correlation</span>
          </div>
          <div className="h-96 bg-secondary/30 rounded-lg p-4 flex items-center justify-center text-muted-foreground">
            <Network className="h-12 w-12 opacity-50" />
            <p className="mt-2">Identity graph visualization - force-directed graph of linked accounts</p>
          </div>
        </div>
      </div>
    </div>
  );
}