import { useState } from "react";
import { useNavigate, Navigate } from "react-router";
import { useAuth } from "../hooks/useAuth";

export function LoginPage() {
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const { login, isAuthenticated } = useAuth();
  const navigate = useNavigate();

  if (isAuthenticated) {
    return <Navigate to="/" replace />;
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    setLoading(true);
    try {
      await login(password);
      navigate("/", { replace: true });
    } catch {
      setError("Invalid password");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="relative flex items-center justify-center min-h-screen overflow-hidden bg-zinc-950">
      {/* Ambient glow */}
      <div className="absolute inset-0 overflow-hidden">
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] bg-primary/5 rounded-full blur-3xl" />
        <div className="absolute top-1/3 left-1/3 w-[400px] h-[400px] bg-accent/5 rounded-full blur-3xl" />
      </div>

      <div className="relative z-10 flex flex-col items-center gap-8">
        {/* Circular logo with glow ring */}
        <div className="relative">
          <div className="absolute inset-0 rounded-full bg-primary/20 blur-xl animate-pulse" />
          <img
            src="/mcclawd-logo.svg"
            alt="McClawd"
            className="relative w-32 h-32 rounded-full"
          />
        </div>

        {/* Title */}
        <h1 className="text-2xl font-light tracking-widest text-zinc-400 uppercase">
          McClawd
        </h1>

        {/* Password form */}
        <form onSubmit={handleSubmit} className="flex flex-col items-center gap-4 w-72">
          <div className="relative w-full">
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Enter master password"
              autoFocus
              className="w-full bg-transparent border-0 border-b border-zinc-700 px-2 py-3 text-center text-zinc-200 placeholder:text-zinc-600 focus:outline-none focus:border-primary transition-colors"
            />
          </div>

          {error && (
            <p className="text-sm text-destructive animate-in fade-in">{error}</p>
          )}

          <button
            type="submit"
            disabled={loading || !password}
            className="w-full py-2.5 rounded-lg bg-primary/10 text-primary border border-primary/20 hover:bg-primary/20 disabled:opacity-40 transition-all text-sm font-medium"
          >
            {loading ? "Unlocking..." : "Unlock"}
          </button>
        </form>
      </div>
    </div>
  );
}
