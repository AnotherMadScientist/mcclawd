import { useState } from "react";
import { useNavigate, Navigate } from "react-router";
import { Fingerprint } from "lucide-react";
import { useAuth } from "../hooks/useAuth";

export function LoginPage() {
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const { login, isAuthenticated, setupComplete } = useAuth();
  const navigate = useNavigate();

  if (isAuthenticated) {
    return <Navigate to="/" replace />;
  }

  if (setupComplete === false) {
    return <Navigate to="/setup" replace />;
  }

  const handleUnlock = async () => {
    setError("");
    setLoading(true);
    try {
      await login();
      navigate("/", { replace: true });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Biometric authentication failed";
      setError(message.includes("cancelled") || message.includes("NotAllowedError")
        ? "Authentication was cancelled"
        : "Biometric authentication failed");
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
            src="/macleod.jpg"
            alt="McClawd"
            className="relative w-32 h-32 rounded-full object-cover"
          />
        </div>

        {/* Title */}
        <h1 className="text-2xl font-light tracking-widest text-zinc-400 uppercase">
          McClawd
        </h1>

        {/* Unlock section */}
        <div className="flex flex-col items-center gap-4 w-72">
          {error && (
            <p className="text-sm text-destructive animate-in fade-in">{error}</p>
          )}

          <button
            type="button"
            onClick={handleUnlock}
            disabled={loading || setupComplete === null}
            className="w-full py-2.5 rounded-lg bg-primary/10 text-primary border border-primary/20 hover:bg-primary/20 disabled:opacity-40 transition-all text-sm font-medium flex items-center justify-center gap-2"
          >
            <Fingerprint className="w-4 h-4" />
            {loading ? "Authenticating..." : "Unlock with Biometric ID"}
          </button>
        </div>
      </div>
    </div>
  );
}
