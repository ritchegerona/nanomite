import { jsx as _jsx } from "react/jsx-runtime";
import { RouterProvider } from '@tanstack/react-router';
import { router } from './app/router';
export function App() {
    return _jsx(RouterProvider, { router: router });
}
