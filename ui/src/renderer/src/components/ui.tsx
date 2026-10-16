import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';

export function Card({ title, children, aside }: { title?: string; children: ReactNode; aside?: ReactNode }) {
	return (
		<section className="card">
			{title || aside ? (
				<header className="card-head">
					{title ? <h2>{title}</h2> : <span />}
					{aside}
				</header>
			) : null}
			{children}
		</section>
	);
}

export function Switch({ checked, onChange, label, hint, disabled }: { checked: boolean; onChange(value: boolean): void; label: string; hint?: string; disabled?: boolean }) {
	return (
		<label className={`switch-row${disabled ? ' disabled' : ''}`}>
			<span className="switch-text">
				<span className="switch-label">{label}</span>
				{hint ? <span className="hint">{hint}</span> : null}
			</span>
			<input type="checkbox" role="switch" checked={checked} disabled={disabled} onChange={event => onChange(event.target.checked)} />
			<span className="switch-track" aria-hidden="true">
				<span className="switch-thumb" />
			</span>
		</label>
	);
}

export function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: { value: T; label: string }[]; onChange(value: T): void; label: string }) {
	return (
		<div className="segmented" role="tablist" aria-label={label}>
			{options.map(option => (
				<button key={option.value} type="button" role="tab" aria-selected={option.value === value} className={option.value === value ? 'on' : ''} onClick={() => onChange(option.value)}>
					{option.label}
				</button>
			))}
		</div>
	);
}

/** Copies text and says so for a moment. */
export function CopyButton({ text, label = 'Copy', disabled }: { text: string; label?: string; disabled?: boolean }) {
	const [copied, setCopied] = useState(false);
	const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
	useEffect(() => () => clearTimeout(timer.current), []);
	const copy = useCallback(() => {
		void navigator.clipboard.writeText(text).then(() => {
			setCopied(true);
			clearTimeout(timer.current);
			timer.current = setTimeout(() => setCopied(false), 1400);
		});
	}, [text]);
	return (
		<button type="button" className="btn small" onClick={copy} disabled={disabled}>
			{copied ? 'Copied' : label}
		</button>
	);
}

export function Field({ label, children, hint }: { label: string; children: ReactNode; hint?: string }) {
	return (
		<div className="field">
			<span className="field-label">{label}</span>
			{children}
			{hint ? <span className="hint">{hint}</span> : null}
		</div>
	);
}
