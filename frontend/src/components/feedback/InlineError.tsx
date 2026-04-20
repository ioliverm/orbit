import { AlertCircle } from 'lucide-react';

interface Props {
  id?: string;
  children: React.ReactNode;
}

// Inline validation error. Pairs with an aria-describedby on the input
// (G-18). Icon + text so color is never the only signal (G-23).
export function InlineError({ id, children }: Props): JSX.Element {
  return (
    <p id={id} className="field__error" role="alert">
      <AlertCircle aria-hidden="true" size={12} />
      <span>{children}</span>
    </p>
  );
}
