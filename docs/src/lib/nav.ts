export interface NavItem {
	href: string;
	label: string;
}

export interface NavSection {
	title: string;
	items: NavItem[];
}

export const nav: NavSection[] = [
	{
		title: 'Getting Started',
		items: [
			{ href: '#overview', label: 'Overview' },
			{ href: '#installation', label: 'Installation' },
			{ href: '#quick-start', label: 'Quick Start' }
		]
	},
	{
		title: 'Server Setup',
		items: [
			{ href: '#prerequisites', label: 'Prerequisites' },
			{ href: '#docker-quickstart', label: 'A. Docker Compose (quickest)' },
			{ href: '#download-binary', label: 'B. Prebuilt binary' },
			{ href: '#run-backend', label: 'C. Manual: run the backend' },
			{ href: '#run-studio', label: 'C. Manual: run Studio' },
			{ href: '#run-sdk', label: 'C. Manual: install the SDK' },
			{ href: '#first-time-setup', label: 'First-time setup' },
			{ href: '#production', label: 'Production release' },
			{ href: '#env-vars', label: 'Environment variables' }
		]
	},
	{
		title: 'Type Safety',
		items: [
			{ href: '#type-safety', label: 'Overview' },
			{ href: '#codegen', label: '1. Codegen (recommended)' },
			{ href: '#hand-written-generics', label: '2. Hand-written generics' },
			{ href: '#runtime-schema', label: '3. Runtime schema' },
			{ href: '#cli-reference', label: 'CLI reference' }
		]
	},
	{
		title: 'Querying Data',
		items: [
			{ href: '#select', label: 'select() — field projection' },
			{ href: '#filter-sort-expand', label: 'filter / sort / expand' }
		]
	},
	{
		title: 'API Reference',
		items: [
			{ href: '#client', label: 'LazypockClient' },
			{ href: '#constructor-options', label: 'Constructor Options' },
			{ href: '#auth-methods', label: 'Authentication' },
			{ href: '#collections-service', label: 'Collections Service' },
			{ href: '#file-operations', label: 'File Operations' },
			{ href: '#realtime-api', label: 'Realtime (low-level)' },
			{ href: '#collection-service', label: 'CollectionService' },
			{ href: '#auth-store', label: 'AuthStore' },
			{ href: '#types', label: 'Types' }
		]
	},
	{
		title: 'Advanced',
		items: [
			{ href: '#auto-cancellation', label: 'Auto Cancellation' },
			{ href: '#error-handling', label: 'Error Handling' },
			{ href: '#configuration', label: 'Configuration' },
			{ href: '#realtime-subscriptions', label: 'Real-time Subscriptions' }
		]
	},
	{
		title: 'More',
		items: [{ href: '#license', label: 'License' }]
	}
];
