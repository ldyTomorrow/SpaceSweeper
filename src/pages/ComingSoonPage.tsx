interface ComingSoonPageProps {
  eyebrow: string;
  title: string;
  description: string;
  phase: string;
}

export function ComingSoonPage({
  eyebrow,
  title,
  description,
  phase,
}: ComingSoonPageProps) {
  return (
    <div className="page">
      <section className="hero">
        <div className="hero-copy">
          <p className="eyebrow">{eyebrow}</p>
          <h1>{title}</h1>
          <p className="lede">{description}</p>
        </div>
      </section>

      <section className="panel coming-soon">
        <span className="coming-soon-phase">{phase}</span>
        <h2>该模块已列入后续阶段</h2>
        <p>
          当前版本先完成大文件与旧文件的发现、定位和废纸篓闭环。
          这个入口会沿用同一套流式扫描、取消、安全白名单和审计机制。
        </p>
      </section>
    </div>
  );
}
