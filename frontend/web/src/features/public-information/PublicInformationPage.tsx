import { BrandLockup, Card } from "../../shared/ui";
import { PublicInformationLinks } from "./PublicInformationLinks";

const pages = {
  terms: {
    title: "Terms of Service",
    placeholder: "The terms of service are being prepared. Final terms will be published here."
  },
  privacy: {
    title: "Privacy Policy",
    placeholder: "The privacy policy is being prepared. Information about data handling and your privacy choices will be published here."
  },
  support: {
    title: "Help and contact",
    placeholder: "Support information and a public contact address will be added here."
  }
};

export function PublicInformationPage({ page }: { page: keyof typeof pages }) {
  const content = pages[page];

  return (
    <main className="h-full overflow-y-auto bg-bg px-6 py-10 text-text">
      <div className="mx-auto w-full max-w-2xl space-y-7">
        <header className="flex flex-wrap items-center justify-between gap-4">
          <BrandLockup className="w-[152px]" />
          <a href="/" className="inline-flex min-h-[var(--ng-min-target-size)] items-center rounded text-sm text-muted underline underline-offset-4 hover:text-text">
            Back to NoteGate
          </a>
        </header>

        <Card className="p-6 sm:p-8">
          <p className="mb-3 text-xs font-medium text-muted">In preparation</p>
          <h1 className="text-2xl font-semibold tracking-tight">{content.title}</h1>
          <p className="mt-4 text-sm leading-7 text-muted">{content.placeholder}</p>
        </Card>

        <PublicInformationLinks />
      </div>
    </main>
  );
}
