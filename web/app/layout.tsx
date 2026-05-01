import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "가계부 통합 뷰어",
  description: "월별 가계부를 통합 조회·분석하는 뷰어",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="ko">
      <body>{children}</body>
    </html>
  );
}
