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

  const { register, handleSubmit, formState: { errors } } = useForm<{
    email: string;
    password: string;
  }>({
    resolver: zodResolver(loginSchema),
  });

  const onSubmit = (_data: { email: string; password: string }) => {
    navigate({ to: '/', search: { team: 'blue' } });
  };

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-background">
      <div className="w-full max-w-md p-8 bg-card border border-border rounded-xl shadow-lg">
        <div className="flex items-center gap-2 mb-6">
          <div className="w-10 h-10 bg-primary rounded-lg flex items-center justify-center">
            <Bot className="h-5 w-5 text-primary-foreground" />
          </div>
          <h1 className="text-2xl font-bold">Nanomite</h1>
          <span className="ml-auto text-xs font-mono text-muted-foreground">Command Center</span>
        </div>

        <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
          <div className="space-y-2">
            <label className="text-sm font-medium">Email</label>
            <input
              {...register('email')}
              type="email"
              placeholder="you@company.com"
              className="w-full px-3 py-2 border rounded-lg focus:ring-2 focus:ring-primary outline-none"
            />
            {errors.email && <span className="text-xs text-destructive">{errors.email.message}</span>}
          </div>

          <div className="space-y-2">
            <label className="text-sm font-medium">Password</label>
            <input
              {...register('password')}
              type="password"
              placeholder="••••••••"
              className="w-full px-3 py-2 border rounded-lg focus:ring-2 focus:ring-primary outline-none"
            />
            {errors.password && <span className="text-xs text-destructive">{errors.password.message}</span>}
          </div>

          <button
            type="submit"
            className="w-full py-3 bg-primary text-white rounded-lg hover:bg-primary/90 transition-colors font-medium text-sm"
          >
            Sign In
          </button>
        </form>

        <div className="mt-6 border-t border-border pt-6">
          <p className="text-sm text-muted-foreground mb-4">
            {isRegister ? 'Already have an account?' : "Don't have an account?"}
            <button
              onClick={() => setIsRegister(!isRegister)}
              className="text-primary hover:underline ml-1 font-medium"
            >
              {isRegister ? 'Sign in' : 'Create account'}
            </button>
          </p>

          {!isRegister && (
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <Lock className="h-4 w-4 text-muted-foreground" />
                <span className="text-sm text-muted-foreground">Single Sign-On (OIDC)</span>
              </div>
              <div className="flex items-center justify-between pt-3">
                <span className="text-sm text-muted-foreground">Multi-factor authentication</span>
              </div>
              <div className="flex items-center justify-between pt-3">
                <span className="text-sm text-muted-foreground">Role-based access control</span>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}