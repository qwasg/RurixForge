import AboutPanel from '@/components/shell/AboutPanel';
import { SetH1 } from './controls';

/** 设置·关于页:内容与 Help → 关于 弹窗共用 AboutPanel(版本与服务状态全部实测)。 */
export default function AboutPage() {
  return (
    <div data-testid="settings-page-about" className="flex flex-col">
      <SetH1>关于</SetH1>
      <AboutPanel />
    </div>
  );
}
