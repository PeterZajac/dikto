export default function SettingsPage() {
  return (
    <div className="page">
      <svg className="page__icon" viewBox="0 0 20 20" width="36" height="36" fill="none" aria-hidden>
        <circle cx="8" cy="5.5" r="1.7" stroke="currentColor" strokeWidth="1.3" />
        <path d="M3 5.5h4M11.5 5.5H17" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
        <circle cx="13" cy="10" r="1.7" stroke="currentColor" strokeWidth="1.3" />
        <path d="M3 10h8.5M14.7 10H17" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
        <circle cx="6.5" cy="14.5" r="1.7" stroke="currentColor" strokeWidth="1.3" />
        <path d="M3 14.5h1.9M8.2 14.5H17" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
      </svg>
      <h1 className="page__title">Nastavenia</h1>
      <p className="page__hint">
        Klávesová skratka, jazyk, čistenie textu a Groq kľúč čoskoro pribudnú tu.
      </p>
    </div>
  );
}
