import {
  createContext,
  useContext,
  useState,
  useCallback,
  useEffect,
  type ReactNode,
} from "react";
import { startRegistration, startAuthentication } from "@simplewebauthn/browser";
import { api, setToken, clearToken, getToken } from "../api/client";

interface AuthContextType {
  isAuthenticated: boolean;
  setupComplete: boolean | null; // null = loading
  register: () => Promise<void>;
  login: () => Promise<void>;
  logout: () => void;
}

const AuthContext = createContext<AuthContextType | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [isAuthenticated, setIsAuthenticated] = useState(!!getToken());
  const [setupComplete, setSetupComplete] = useState<boolean | null>(null);

  useEffect(() => {
    api.auth
      .status()
      .then(({ setup_complete }) => setSetupComplete(setup_complete))
      .catch(() => setSetupComplete(false));
  }, []);

  const register = useCallback(async () => {
    const raw = await api.auth.registerStart();
    // webauthn-rs wraps options in { publicKey: {...} } — unwrap for @simplewebauthn/browser
    const options = (raw as any).publicKey ?? raw;
    const credential = await startRegistration({ optionsJSON: options });
    const { token } = await api.auth.registerFinish(credential);
    setToken(token);
    setSetupComplete(true);
    setIsAuthenticated(true);
  }, []);

  const login = useCallback(async () => {
    const raw = await api.auth.loginStart();
    // webauthn-rs wraps options in { publicKey: {...} } — unwrap for @simplewebauthn/browser
    const options = (raw as any).publicKey ?? raw;
    const credential = await startAuthentication({ optionsJSON: options });
    const { token } = await api.auth.loginFinish(credential);
    setToken(token);
    setIsAuthenticated(true);
  }, []);

  const logout = useCallback(() => {
    clearToken();
    setIsAuthenticated(false);
  }, []);

  return (
    <AuthContext.Provider value={{ isAuthenticated, setupComplete, register, login, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

// Safe default for HMR edge case where Layout renders before AuthProvider remounts
const AUTH_DEFAULT: AuthContextType = {
  isAuthenticated: false,
  setupComplete: null,
  register: async () => {},
  login: async () => {},
  logout: () => {},
};

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) {
    // During Vite HMR, components may briefly render outside AuthProvider.
    // Return safe default instead of throwing to avoid cascading errors.
    if (import.meta.env.DEV) {
      console.warn("useAuth called outside AuthProvider (HMR race) — using safe default");
      return AUTH_DEFAULT;
    }
    throw new Error("useAuth must be used within AuthProvider");
  }
  return ctx;
}
