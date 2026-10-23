import { AuthPage } from "../../auth/AuthPage";
import { Notice } from "../../shared/ui";

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
    <AuthPage title={content.title} wide footer={<a href="/">Back to NoteGate</a>}>
      <Notice title="In preparation"><p>{content.placeholder}</p></Notice>
    </AuthPage>
  );
}
