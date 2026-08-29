import { sdkNav } from "./sdk-nav.generated";

export interface NavItem {
	href: string;
	label: string;
	badge?: string;
	children?: NavItem[];
}

export interface NavSection {
	title: string;
	items: NavItem[];
}

export const nav: NavSection[] = [
	{
		title: "Getting Started",
		items: [{ href: "/", label: "What is Lazypock" }],
	},
	{
		title: "Server",
		items: [
			{
				href: "/server",
				label: "Server Guide",
				children: [
					{ href: "/server#quick-start", label: "Quick Start" },
					{ href: "/server#binary", label: "Prebuilt binary" },
					{ href: "/server#manual", label: "Manual setup" },
					{ href: "/server#first-time", label: "First-time setup" },
					{ href: "/server#production", label: "Production" },
					{ href: "/server#env-vars", label: "Environment variables" },
				],
			},
		],
	},
	{
		title: "SDKs",
		items: [
			{ href: "/sdk", label: "Overview" },
			...sdkNav.map((sdk) => ({
				href: `/sdk/${sdk.slug}`,
				label: sdk.name,
				children: sdk.pages.map((p) => ({
					href: `/sdk/${sdk.slug}/${p.slug}`,
					label: p.label,
				})),
			})),
			{ href: "/sdk/swift", label: "Swift", badge: "coming soon" },
		],
	},
	{
		title: "More",
		items: [{ href: "/#license", label: "License" }],
	},
];
