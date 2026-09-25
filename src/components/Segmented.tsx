export function Segmented<T extends string>(props: {
  value: T;
  options: { value: T; label: string; title?: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segmented" role="radiogroup">
      {props.options.map((o) => (
        <button
          key={o.value}
          role="radio"
          aria-checked={o.value === props.value}
          className={o.value === props.value ? "on" : undefined}
          title={o.title}
          onClick={() => o.value !== props.value && props.onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
