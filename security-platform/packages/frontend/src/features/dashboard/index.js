import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { Activity, AlertTriangle, Bot, ShieldAlert, Terminal } from 'lucide-react';
export function DashboardPage() {
    const stats = {
        activeAssessments: 3,
        openFindings: 12,
        runningToolRuns: 5,
        siemAlerts: 23,
        modules: [
            { name: 'OSINT', count: 45, trend: '+12%', color: 'text-blue-500' },
            { name: 'Pentesting', count: 8, trend: '+3%', color: 'text-red-500' },
            { name: 'Vulnerability Scan', count: 23, trend: '-5%', color: 'text-green-500' },
            { name: 'SIEM', count: 156, trend: '+2%', color: 'text-purple-500' },
            { name: 'SocMed', count: 12, trend: '+8%', color: 'text-yellow-500' },
        ],
        recentActivity: [
            { action: 'Subdomain scan completed', module: 'osint', time: '2m ago' },
            { action: 'New SIEM alert: T1059', module: 'siem', time: '15m ago' },
            { action: 'SocMed investigation started', module: 'socmed', time: '1h ago' },
            { action: 'Vulnerability scan finished', module: 'vulnerability_scanning', time: '3h ago' },
        ],
    };
    return (_jsxs("div", { className: "space-y-6", children: [_jsxs("div", { className: "flex flex-col md:flex-row md:items-center md:justify-between gap-4", children: [_jsxs("div", { children: [_jsx("h1", { className: "text-2xl font-bold", children: "Command Center" }), _jsx("p", { className: "text-muted-foreground", children: "Unified security operations for Blue, Red & Purple teams" })] }), _jsxs("div", { className: "flex gap-2", children: [_jsxs("button", { className: "px-4 py-2 bg-primary text-white rounded-lg hover:bg-primary/90 transition-colors text-sm font-medium flex items-center gap-2", children: [_jsx(Bot, { className: "h-4 w-4" }), "New Assessment"] }), _jsx("button", { className: "px-4 py-2 bg-secondary text-muted-foreground rounded-lg hover:bg-secondary/80 transition-colors text-sm font-medium", children: "Export Report" })] })] }), _jsxs("div", { className: "grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4", children: [_jsx(StatCard, { title: "Active Assessments", value: stats.activeAssessments, icon: Activity, color: "text-blue-500", bg: "bg-blue-500/10" }), _jsx(StatCard, { title: "Open Findings", value: stats.openFindings, icon: AlertTriangle, color: "text-red-500", bg: "bg-red-500/10" }), _jsx(StatCard, { title: "Running Tool Runs", value: stats.runningToolRuns, icon: Terminal, color: "text-green-500", bg: "bg-green-500/10" }), _jsx(StatCard, { title: "SIEM Alerts", value: stats.siemAlerts, icon: ShieldAlert, color: "text-purple-500", bg: "bg-purple-500/10" })] }), _jsx("div", { className: "grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4", children: stats.modules.map((m) => (_jsx(ModuleStatCard, { module: m }, m.name))) }), _jsx("div", { className: "lg:col-span-3 xl:col-span-4", children: _jsxs("div", { className: "border rounded-lg bg-card p-6", children: [_jsxs("div", { className: "flex items-center justify-between mb-4", children: [_jsx("h2", { className: "text-lg font-semibold", children: "Recent Activity" }), _jsx("a", { href: "#", className: "text-sm text-primary hover:underline", children: "View all" })] }), _jsx("div", { className: "space-y-3", children: stats.recentActivity.map((a, i) => (_jsxs("div", { className: "flex items-center gap-3 p-3 rounded-lg hover:bg-secondary/50 transition-colors border border-transparent hover:border-border", children: [_jsx("div", { className: "w-8 h-8 rounded flex items-center justify-center text-xs font-bold bg-secondary/50 text-muted-foreground", children: i % 4 === 0 ? 'O' : i % 4 === 1 ? 'S' : i % 4 === 2 ? 'V' : 'P' }), _jsxs("div", { className: "flex-1", children: [_jsx("div", { className: "text-sm font-medium", children: a.action }), _jsxs("div", { className: "text-xs text-muted-foreground flex items-center gap-1", children: [_jsx("span", { className: "text-[10px] px-1.5 py-0.5 rounded bg-primary/10 text-primary", children: a.module }), _jsx("span", { children: a.time })] })] })] }, i))) })] }) })] }));
}
function StatCard({ title, value, icon: Icon, color, bg }) {
    return (_jsx("div", { className: ['bg-card rounded-lg p-5 border border-border', bg].join(' '), children: _jsxs("div", { className: "flex items-center justify-between", children: [_jsxs("div", { children: [_jsx("p", { className: "text-sm text-muted-foreground", children: title }), _jsx("p", { className: "text-2xl font-bold", children: value })] }), _jsx("div", { className: `w-10 h-10 rounded-lg ${bg} flex items-center justify-center ${color}`, children: _jsx(Icon, { className: "h-5 w-5" }) })] }) }));
}
function ModuleStatCard({ module }) {
    return (_jsxs("div", { className: "bg-card rounded-lg border border-border p-4", children: [_jsxs("div", { className: "flex items-center justify-between mb-2", children: [_jsx("h3", { className: "font-semibold", children: module.name }), _jsx("span", { className: ['text-xs font-medium px-2 py-1 rounded bg-secondary/50 text-muted-foreground', module.color].join(' '), children: module.trend })] }), _jsx("div", { className: "text-3xl font-bold", children: module.count }), _jsx("div", { className: "mt-2 h-1.5 bg-secondary rounded-full overflow-hidden", children: _jsx("div", { className: "h-full bg-primary rounded-full", style: { width: '65%' } }) })] }));
}
