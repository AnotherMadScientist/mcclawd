import { useState } from "react";
import { useNavigate, Navigate } from "react-router";
import { Fingerprint } from "lucide-react";
import { useAuth } from "../hooks/useAuth";

export function SetupPage() {
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const { register, isAuthenticated, setupComplete } = useAuth();
  const navigate = useNavigate();

  if (isAuthenticated) {
    return <Navigate to="/" replace />;
  }

  if (setupComplete === true) {
    return <Navigate to="/login" replace />;
  }

  const handleSetup = async () => {
    setError("");
    setLoading(true);
    try {
      await register();
      navigate("/", { replace: true });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Biometric setup failed";
      setError(message.includes("cancelled") || message.includes("NotAllowedError")
        ? "Setup was cancelled"
        : "Failed to set up biometric authentication");
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

        {/* Title + subtitle */}
        <div className="flex flex-col items-center gap-2">
          <h1 className="text-2xl font-light tracking-widest text-zinc-400 uppercase">
            Welcome to McClawd
          </h1>
          <p className="text-sm text-zinc-600 tracking-wide">
            Set up biometric authentication
          </p>
        </div>

        {/* Setup section */}
        <div className="flex flex-col items-center gap-4 w-72">
          {error && (
            <p className="text-sm text-destructive animate-in fade-in">{error}</p>
          )}

          <button
            type="button"
            onClick={handleSetup}
            disabled={loading || setupComplete === null}
            className="w-full py-2.5 rounded-lg bg-primary/10 text-primary border border-primary/20 hover:bg-primary/20 disabled:opacity-40 transition-all text-sm font-medium flex items-center justify-center gap-2"
          >
            <Fingerprint className="w-4 h-4" />
            {loading ? "Setting up..." : "Set up Biometric ID"}
          </button>
        </div>
      </div>
    </div>
  );
}
