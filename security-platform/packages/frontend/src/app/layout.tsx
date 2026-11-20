import { useState, useEffect } from 'react';
import { Bot, Lock, LogOut, Menu, Search, User } from 'lucide-react';

export function LayoutShell({ children }: { children: React.ReactNode }) {
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [agentPanelOpen, setAgentPanelOpen] = useState(true);
  const [theme, setTheme] = useState<'light' | 'dark'>('dark');

  useEffect(() => {
    const saved = localStorage.getItem('theme');
    if (saved === 'light' || saved === 'dark') {
      setTheme(saved as any);
    } else if (window.matchMedia('(prefers-color-scheme: dark)').matches) {
      setTheme('dark');
    }
    document.documentElement.classList.toggle('dark', theme === 'dark');
  }, [theme]);

  const asideClass = sidebarOpen ? 'translate-x-0' : '-translate-x-full';
  const mainClass = 'flex-1 flex flex-col overflow-hidden ' + (sidebarOpen ? 'ml-64' : 'ml-0');
  const mainPanelClass = 'flex-1 overflow-y-auto bg-background p-6 ' + (agentPanelOpen ? 'pr-[420px]' : '');

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <aside className={'fixed left-0 top-0 bottom-0 z-20 w-64 bg-card border-r border-border transition-all duration-300 ' + asideClass}>
        <div className="flex flex-col h-full">
          <div className="p-4 border-b border-border flex items-center gap-2">
            <div className="w-8 h-8 bg-primary rounded-lg flex items-center justify-center">
              <Bot className="h-5 w-5 text-primary-foreground" />
            </div>
            <h1 className="font-bold text-xl tracking-tight">Nanomite</h1>
            <span className="ml-auto text-xs font-mono text-muted-foreground">CMD</span>
          </div>
          <div className="flex-1 overflow-y-auto p-2 space-y-1">
            <div className="flex items-center gap-2 px-3 py-2 mb-2 bg-secondary/50 rounded-lg">
              <Lock className="h-4 w-4" />
              <span className="text-xs font-medium text-muted-foreground">Team:</span>
              <div className="ml-auto flex gap-1">
                <button key="all" className="px-2 py-1 text-xs rounded transition-colors bg-primary text-primary-foreground">
                  ALL
                </button>
                <button key="blue" className="px-2 py-1 text-xs rounded transition-colors text-muted-foreground hover:bg-secondary">
                  BLUE
                </button>
                <button key="red" className="px-2 py-1 text-xs rounded transition-colors text-muted-foreground hover:bg-secondary">
                  RED
                </button>
                <button key="purple" className="px-2 py-1 text-xs rounded transition-colors text-muted-foreground hover:bg-secondary">
                  PURPLE
                </button>
              </div>
            </div>
          </div>
          <div className="p-4 border-t border-border">
            <button
              onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
              className="w-full flex items-center gap-3 px-3 py-2 rounded-lg hover:bg-secondary transition-colors"
            >
              <div className={'w-4 h-4 rounded-full ' + (theme === 'dark' ? 'bg-yellow-400' : 'bg-blue-400')} />
              <span className="text-sm">{theme === 'dark' ? 'Light Mode' : 'Dark Mode'}</span>
            </button>
            <button
              onClick={() => {}}
              className="w-full flex items-center gap-3 px-3 py-2 mt-2 rounded-lg hover:bg-secondary transition-colors text-destructive"
            >
              <LogOut className="h-4 w-4" />
              <span className="text-sm">Sign Out</span>
            </button>
          </div>
        </div>
      </aside>
      <div className={mainClass}>
        <header className="h-16 border-b border-border bg-card/50 backdrop-blur flex items-center justify-between px-6 z-10">
          <div className="flex items-center gap-4">
            <button
              onClick={() => setSidebarOpen(!sidebarOpen)}
              className="p-2 hover:bg-secondary rounded-lg transition-colors"
            >
              <Menu className="h-5 w-5" />
            </button>
            <div className="flex flex-col">
              <h2 className="text-sm font-semibold">Nanomite Command Center</h2>
              <p className="text-xs text-muted-foreground">Unified Security Operations Platform</p>
            </div>
          </div>
          <div className="flex items-center gap-4">
            <button
              className="p-2 hover:bg-secondary rounded-lg transition-colors hidden md:block"
              title="Command Palette (Ctrl+K)"
            >
              <Search className="h-5 w-5" />
            </button>
            <button
              onClick={() => setAgentPanelOpen(!agentPanelOpen)}
              className="p-2 hover:bg-secondary rounded-lg transition-colors"
              title="Toggle Nanomite Agent"
            >
              <Bot className="h-5 w-5" />
            </button>
            <div className="relative">
              <User className="h-6 w-6 rounded-full bg-primary text-primary-foreground" />
            </div>
          </div>
        </header>
        <div className="flex-1 flex overflow-hidden relative">
          <main className={mainPanelClass}>
            {children}
          </main>
          {agentPanelOpen && (
            <aside className="fixed right-0 top-0 bottom-0 w-[420px] bg-card border-l border-border z-30 flex flex-col shadow-lg">
              <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-card/50">
                <div className="flex items-center gap-2">
                  <Bot className="h-5 w-5 text-primary" />
                  <h3 className="font-semibold text-sm">Nanomite Agent</h3>
                </div>
                <div className="flex items-center gap-1">
                  <span className="w-2 h-2 rounded-full bg-green-500" />
                  <span className="text-xs text-muted-foreground">Online</span>
                </div>
              </div>
              <div className="flex-1 overflow-y-auto p-4 space-y-4">
                <div className="space-y-2">
                  <p className="text-xs text-muted-foreground">Welcome back, Operator</p>
                  <p className="text-sm">How can I help with your assessment today?</p>
                </div>
                <div className="border rounded-lg p-3 bg-secondary/30">
                  <h4 className="text-xs font-semibold mb-2">Quick Actions</h4>
                  <div className="grid grid-cols-2 gap-2">
                    <button className="text-xs px-2 py-1.5 bg-primary/10 text-primary rounded hover:bg-primary/20 transition-colors">
                      New Chat
                    </button>
                    <button className="text-xs px-2 py-1.5 bg-secondary/10 text-muted-foreground rounded hover:bg-secondary/20 transition-colors">
                      Load Plan
                    </button>
                  </div>
                </div>
              </div>
              <div className="border-t border-border p-3 bg-card">
                <form className="flex gap-2" onSubmit={(e) => e.preventDefault()}>
                  <input
                    type="text"
                    placeholder="Ask Nanomite..."
                    className="flex-1 px-3 py-2 rounded-lg border focus:ring-2 focus:ring-primary outline-none text-sm"
                  />
                  <button className="px-4 py-2 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 transition-colors text-sm font-medium">
                    Send
                  </button>
                </form>
              </div>
            </aside>
          )}
        </div>
        <footer className="h-8 border-t border-border bg-card/50 backdrop-blur flex items-center justify-between px-4 text-xs text-muted-foreground z-10">
          <div className="flex items-center gap-4">
            <span>Assessments: <span className="font-mono">0 Active</span></span>
            <span>•</span>
            <span>Findings: <span className="font-mono text-red-500">0 Open</span></span>
            <span>•</span>
            <span>Tool Runs: <span className="font-mono text-blue-500">0 Running</span></span>
          </div>
          <div className="flex items-center gap-4">
            <span className="flex items-center gap-1">
              <div className="w-2 h-2 rounded-full bg-blue-500" />
              <span>System: OK</span>
            </span>
            <span>v1.0.0</span>
          </div>
        </footer>
      </div>
    </div>
  );
}