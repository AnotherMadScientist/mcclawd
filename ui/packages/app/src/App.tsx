import { BrowserRouter, Routes, Route } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AuthProvider } from "./hooks/useAuth";
import { Layout } from "./components/Layout";

function Placeholder({ name }: { name: string }) {
  return (
    <div className="flex items-center justify-center h-64">
      <p className="text-muted-foreground text-lg">{name} — coming soon</p>
    </div>
  );
}

function LoginPlaceholder() {
  return (
    <div className="flex items-center justify-center min-h-screen">
      <p className="text-muted-foreground">Login page — coming in Task 11</p>
    </div>
  );
}

const queryClient = new QueryClient();

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <BrowserRouter>
          <Routes>
            <Route path="/login" element={<LoginPlaceholder />} />
            <Route element={<Layout />}>
              <Route index element={<Placeholder name="Tasks" />} />
              <Route path="tasks/new" element={<Placeholder name="New Task" />} />
              <Route path="tasks/:id" element={<Placeholder name="Task Detail" />} />
              <Route path="config/workspace" element={<Placeholder name="Workspace" />} />
              <Route path="config/skills" element={<Placeholder name="Skills" />} />
              <Route path="config/mcp" element={<Placeholder name="MCP Servers" />} />
              <Route path="config/secrets" element={<Placeholder name="Secrets" />} />
              <Route path="config/settings" element={<Placeholder name="Settings" />} />
            </Route>
          </Routes>
        </BrowserRouter>
      </AuthProvider>
    </QueryClientProvider>
  );
}
