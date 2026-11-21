import { createRouter, createRoute } from '@tanstack/react-router';
import { LayoutShell } from './layout';
import { DashboardPage } from '../features/dashboard';
import { LoginPage } from '../features/auth';
import { OSINTSpace } from '../features/osint';
import { PentestSpace } from '../features/pentest';
import { VulnScanSpace } from '../features/vulnscan';
import { SIEMKeySpace } from '../features/siem';
import { SocMedSpace } from '../features/socmed';
export const rootRoute = createRoute({
    component: LayoutShell,
    id: 'root',
    path: '/',
});
rootRoute.addChildren([
    createRoute({ id: 'index', component: DashboardPage, path: '/' }),
    createRoute({ id: 'login', component: LoginPage, path: '/login' }),
    createRoute({ id: 'osint', component: OSINTSpace, path: '/osint' }),
    createRoute({ id: 'pentest', component: PentestSpace, path: '/pentest' }),
    createRoute({ id: 'vulnscan', component: VulnScanSpace, path: '/vulnscan' }),
    createRoute({ id: 'siem', component: SIEMKeySpace, path: '/siem' }),
    createRoute({ id: 'socmed', component: SocMedSpace, path: '/socmed' }),
]);
export const router = createRouter({
    routeTree: rootRoute,
    context: {},
});
