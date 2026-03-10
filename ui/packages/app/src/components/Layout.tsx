import { Outlet, Navigate, useLocation } from "react-router";
import { Sidebar } from "./Sidebar";
import { CommandBar } from "./CommandBar";
import { useAuth } from "../hooks/useAuth";

export function Layout() {
  const { isAuthenticated } = useAuth();
  const location = useLocation();

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />;
  }

  // Hide CommandBar on pages that have their own input (new task, task detail)
  const hideCommandBar =
    location.pathname === "/tasks/new" ||
    /^\/tasks\/[^/]+$/.test(location.pathname);

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar />
      <div className="flex flex-1 flex-col overflow-hidden">
        <main className="relative flex-1 overflow-y-auto p-8">
          <Outlet />
        </main>
        {!hideCommandBar && <CommandBar />}
      </div>
    </div>
  );
}
