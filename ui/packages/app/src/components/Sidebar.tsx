import { NavLink } from "react-router";
import {
  LayoutDashboard,
  FileText,
  Puzzle,
  Server,
  KeyRound,
  Settings,
  ChevronDown,
  LogOut,
  Container,
} from "lucide-react";
import { useState } from "react";
import { cn } from "../lib/utils";
import { useAuth } from "../hooks/useAuth";

const configItems = [
  { to: "/config/workspace", icon: FileText, label: "Workspace" },
  { to: "/config/skills", icon: Puzzle, label: "Skills" },
  { to: "/config/mcp", icon: Server, label: "MCP Servers" },
  { to: "/config/secrets", icon: KeyRound, label: "Secrets" },
  { to: "/config/settings", icon: Settings, label: "Settings" },
  { to: "/config/docker", icon: Container, label: "Docker" },
];

export function Sidebar() {
  const [configOpen, setConfigOpen] = useState(true);
  const { logout } = useAuth();

  return (
    <aside className="flex flex-col w-64 border-r border-border bg-zinc-950 h-screen">
      {/* Logo */}
      <div className="flex items-center gap-3 px-6 py-5 border-b border-border">
        <img src="/macleod.jpg" alt="McClawd" className="w-8 h-8 rounded-full object-cover" />
        <span className="text-lg font-semibold tracking-tight">McClawd</span>
      </div>

      {/* Nav */}
      <nav className="flex-1 px-3 py-4 space-y-1 overflow-y-auto">
        <NavLink
          to="/"
          end
          className={({ isActive }) =>
            cn(
              "flex items-center gap-3 px-3 py-2 rounded-md text-sm font-medium transition-colors",
              isActive
                ? "bg-primary/10 text-primary"
                : "text-muted-foreground hover:bg-muted hover:text-foreground",
            )
          }
        >
          <LayoutDashboard className="w-4 h-4" />
          Tasks
        </NavLink>

        {/* Configuration section */}
        <button
          onClick={() => setConfigOpen(!configOpen)}
          className="flex items-center justify-between w-full px-3 py-2 rounded-md text-sm font-medium text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
        >
          <span className="flex items-center gap-3">
            <Settings className="w-4 h-4" />
            Configuration
          </span>
          <ChevronDown
            className={cn("w-4 h-4 transition-transform", configOpen && "rotate-180")}
          />
        </button>

        {configOpen && (
          <div className="ml-4 space-y-1">
            {configItems.map(({ to, icon: Icon, label }) => (
              <NavLink
                key={to}
                to={to}
                className={({ isActive }) =>
                  cn(
                    "flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors",
                    isActive
                      ? "bg-primary/10 text-primary"
                      : "text-muted-foreground hover:bg-muted hover:text-foreground",
                  )
                }
              >
                <Icon className="w-4 h-4" />
                {label}
              </NavLink>
            ))}
          </div>
        )}
      </nav>

      {/* Footer */}
      <div className="px-3 py-4 border-t border-border">
        <button
          onClick={logout}
          className="flex items-center gap-3 px-3 py-2 w-full rounded-md text-sm text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
        >
          <LogOut className="w-4 h-4" />
          Sign Out
        </button>
      </div>
    </aside>
  );
}
