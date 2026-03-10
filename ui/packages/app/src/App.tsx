import { BrowserRouter, Routes, Route } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AuthProvider } from "./hooks/useAuth";
import { Layout } from "./components/Layout";
import { LoginPage } from "./pages/LoginPage";
import { SetupPage } from "./pages/SetupPage";
import { TasksPage } from "./pages/TasksPage";
import { NewTaskPage } from "./pages/NewTaskPage";
import { TaskDetailPage } from "./pages/TaskDetailPage";
import { WorkspacePage } from "./pages/WorkspacePage";
import { SkillsPage } from "./pages/SkillsPage";
import { McpServersPage } from "./pages/McpServersPage";
import { SecretsPage } from "./pages/SecretsPage";
import { SettingsPage } from "./pages/SettingsPage";
import { DockerPage } from "./pages/DockerPage";
import { UsagePage } from "./pages/UsagePage";
import { SecurityEventsPage } from "./pages/SecurityEventsPage";
import { SecurityRulesPage } from "./pages/SecurityRulesPage";
import { WorldNewsPage } from "./pages/WorldNewsPage";

const queryClient = new QueryClient();

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <BrowserRouter>
          <Routes>
            <Route path="/login" element={<LoginPage />} />
            <Route path="/setup" element={<SetupPage />} />
            <Route element={<Layout />}>
              <Route index element={<WorldNewsPage />} />
              <Route path="tasks" element={<TasksPage />} />
              <Route path="tasks/new" element={<NewTaskPage />} />
              <Route path="tasks/:id" element={<TaskDetailPage />} />
              <Route path="config/workspace" element={<WorkspacePage />} />
              <Route path="config/skills" element={<SkillsPage />} />
              <Route path="config/mcp" element={<McpServersPage />} />
              <Route path="config/secrets" element={<SecretsPage />} />
              <Route path="config/settings" element={<SettingsPage />} />
              <Route path="config/docker" element={<DockerPage />} />
              <Route path="config/usage" element={<UsagePage />} />
              <Route path="config/security/events" element={<SecurityEventsPage />} />
              <Route path="config/security/rules" element={<SecurityRulesPage />} />
            </Route>
          </Routes>
        </BrowserRouter>
      </AuthProvider>
    </QueryClientProvider>
  );
}
