import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Root Signal Admin",
  description: "Admin dashboard for the Root Signal platform",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en">
      <head>
        <link
          href="https://api.mapbox.com/mapbox-gl-js/v3.18.1/mapbox-gl.css"
          rel="stylesheet"
        />
      </head>
      <body className="bg-gray-50 text-gray-900 antialiased">{children}</body>
    </html>
  );
}
