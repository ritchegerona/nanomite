import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { useState } from 'react';
import { Lock, Bot } from 'lucide-react';
import { useForm } from 'react-hook-form';
import { z } from 'zod';
import { zodResolver } from '@hookform/resolvers/zod';
import { useNavigate } from '@tanstack/react-router';
const loginSchema = z.object({
    email: z.string().email(),
    password: z.string().min(1),
});
export function LoginPage() {
    const [isRegister, setIsRegister] = useState(false);
    const navigate = useNavigate();
    const { register, handleSubmit, formState: { errors } } = useForm({
        resolver: zodResolver(loginSchema),
    });
    const onSubmit = (_data) => {
        navigate({ to: '/', search: { team: 'blue' } });
    };
    return (_jsx("div", { className: "flex h-screen w-screen items-center justify-center bg-background", children: _jsxs("div", { className: "w-full max-w-md p-8 bg-card border border-border rounded-xl shadow-lg", children: [_jsxs("div", { className: "flex items-center gap-2 mb-6", children: [_jsx("div", { className: "w-10 h-10 bg-primary rounded-lg flex items-center justify-center", children: _jsx(Bot, { className: "h-5 w-5 text-primary-foreground" }) }), _jsx("h1", { className: "text-2xl font-bold", children: "Nanomite" }), _jsx("span", { className: "ml-auto text-xs font-mono text-muted-foreground", children: "Command Center" })] }), _jsxs("form", { onSubmit: handleSubmit(onSubmit), className: "space-y-4", children: [_jsxs("div", { className: "space-y-2", children: [_jsx("label", { className: "text-sm font-medium", children: "Email" }), _jsx("input", { ...register('email'), type: "email", placeholder: "you@company.com", className: "w-full px-3 py-2 border rounded-lg focus:ring-2 focus:ring-primary outline-none" }), errors.email && _jsx("span", { className: "text-xs text-destructive", children: errors.email.message })] }), _jsxs("div", { className: "space-y-2", children: [_jsx("label", { className: "text-sm font-medium", children: "Password" }), _jsx("input", { ...register('password'), type: "password", placeholder: "\u2022\u2022\u2022\u2022\u2022\u2022\u2022\u2022", className: "w-full px-3 py-2 border rounded-lg focus:ring-2 focus:ring-primary outline-none" }), errors.password && _jsx("span", { className: "text-xs text-destructive", children: errors.password.message })] }), _jsx("button", { type: "submit", className: "w-full py-3 bg-primary text-white rounded-lg hover:bg-primary/90 transition-colors font-medium text-sm", children: "Sign In" })] }), _jsxs("div", { className: "mt-6 border-t border-border pt-6", children: [_jsxs("p", { className: "text-sm text-muted-foreground mb-4", children: [isRegister ? 'Already have an account?' : "Don't have an account?", _jsx("button", { onClick: () => setIsRegister(!isRegister), className: "text-primary hover:underline ml-1 font-medium", children: isRegister ? 'Sign in' : 'Create account' })] }), !isRegister && (_jsxs("div", { className: "space-y-3", children: [_jsxs("div", { className: "flex items-center justify-between", children: [_jsx(Lock, { className: "h-4 w-4 text-muted-foreground" }), _jsx("span", { className: "text-sm text-muted-foreground", children: "Single Sign-On (OIDC)" })] }), _jsx("div", { className: "flex items-center justify-between pt-3", children: _jsx("span", { className: "text-sm text-muted-foreground", children: "Multi-factor authentication" }) }), _jsx("div", { className: "flex items-center justify-between pt-3", children: _jsx("span", { className: "text-sm text-muted-foreground", children: "Role-based access control" }) })] }))] })] }) }));
}
