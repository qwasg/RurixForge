import { BrowserRouter, Route, Routes } from 'react-router-dom';
import { AppLayout } from '@/components/layout/AppLayout';
import { RequireAuth } from '@/components/layout/RequireAuth';
import { AuthProvider } from '@/lib/auth';
import { SettingsProvider } from '@/lib/settings';
import { ThemeProvider } from '@/lib/theme';
import { ToastProvider } from '@/lib/toast';
import { AccountsPage } from '@/pages/accounts/AccountsPage';
import { AuditLogsPage } from '@/pages/AuditLogsPage';
import { DashboardPage } from '@/pages/DashboardPage';
import { GroupsPage } from '@/pages/GroupsPage';
import { LoginPage } from '@/pages/LoginPage';
import { ModelsPage } from '@/pages/models/ModelsPage';
import { NotFoundPage } from '@/pages/NotFoundPage';
import { OrdersPage } from '@/pages/OrdersPage';
import { PlansPage } from '@/pages/PlansPage';
import { RedeemCodesPage } from '@/pages/RedeemCodesPage';
import { SettingsPage } from '@/pages/SettingsPage';
import { UsagePage } from '@/pages/UsagePage';
import { UsersPage } from '@/pages/users/UsersPage';

export const ROUTER_BASENAME = '/admin';

export function AppRoutes() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route
        element={
          <RequireAuth>
            <SettingsProvider>
              <AppLayout />
            </SettingsProvider>
          </RequireAuth>
        }
      >
        <Route index element={<DashboardPage />} />
        <Route path="users" element={<UsersPage />} />
        <Route path="groups" element={<GroupsPage />} />
        <Route path="plans" element={<PlansPage />} />
        <Route path="orders" element={<OrdersPage />} />
        <Route path="accounts" element={<AccountsPage />} />
        <Route path="models" element={<ModelsPage />} />
        <Route path="redeem-codes" element={<RedeemCodesPage />} />
        <Route path="usage" element={<UsagePage />} />
        <Route path="settings" element={<SettingsPage />} />
        <Route path="audit-logs" element={<AuditLogsPage />} />
        <Route path="*" element={<NotFoundPage />} />
      </Route>
    </Routes>
  );
}

export function App() {
  return (
    <ThemeProvider>
      <ToastProvider>
        <AuthProvider>
          <BrowserRouter basename={ROUTER_BASENAME}>
            <AppRoutes />
          </BrowserRouter>
        </AuthProvider>
      </ToastProvider>
    </ThemeProvider>
  );
}
