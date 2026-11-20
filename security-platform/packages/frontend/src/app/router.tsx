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
  component: LayoutShell as any,
  id: 'root',
  path: '/',
} as any);

rootRoute.addChildren([
  createRoute({ id: 'index', component: DashboardPage as any, path: '/' } as any),
  createRoute({ id: 'login', component: LoginPage as any, path: '/login' } as any),
  createRoute({ id: 'osint', component: OSINTSpace as any, path: '/osint' } as any),
  createRoute({ id: 'pentest', component: PentestSpace as any, path: '/pentest' } as any),
  createRoute({ id: 'vulnscan', component: VulnScanSpace as any, path: '/vulnscan' } as any),
  createRoute({ id: 'siem', component: SIEMKeySpace as any, path: '/siem' } as any),
  createRoute({ id: 'socmed', component: SocMedSpace as any, path: '/socmed' } as any),
]);

export const router = (createRouter as any)({
  routeTree: rootRoute,
  context: {},
});