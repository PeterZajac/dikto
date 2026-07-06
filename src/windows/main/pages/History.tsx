export default function HistoryPage() {
  return (
    <div className="page">
      <svg className="page__icon" viewBox="0 0 20 20" width="36" height="36" fill="none" aria-hidden>
        <path
          d="M10 3.5a6.5 6.5 0 1 1 -5.72 3.4"
          stroke="currentColor"
          strokeWidth="1.3"
          strokeLinecap="round"
        />
        <path d="M2.6 3.4v3.9h3.9" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
        <path d="M10 6.7v3.4l2.4 1.5" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      <h1 className="page__title">História</h1>
      <p className="page__hint">Zoznam tvojich doterajších diktovaní sa čoskoro zobrazí tu.</p>
    </div>
  );
}
