import { createContext, useContext, useState, useCallback, type ReactNode } from "react";
import { api, setToken, clearToken, getToken } from "../api/client";

interface AuthContextType {
  isAuthenticated: boolean;
  login: (password: string) => Promise<void>;
  logout: () => void;
}

const AuthContext = createContext<AuthContextType | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [isAuthenticated, setIsAuthenticated] = useState(!!getToken());

  const login = useCallback(async (password: string) => {
    const { token } = await api.auth.login(password);
    setToken(token);
    setIsAuthenticated(true);
  }, []);

  const logout = useCallback(() => {
    clearToken();
    setIsAuthenticated(false);
  }, []);

  return (
    <AuthContext.Provider value={{ isAuthenticated, login, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
