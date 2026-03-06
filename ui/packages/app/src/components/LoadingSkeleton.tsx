export function CardSkeleton() {
  return <div className="animate-pulse rounded-lg bg-muted h-24 w-full" />;
}

export function ListSkeleton({ count = 3 }: { count?: number }) {
  return (
    <div className="space-y-3">
      {Array.from({ length: count }, (_, i) => (
        <CardSkeleton key={i} />
      ))}
    </div>
  );
}

export function FieldSkeleton() {
  return (
    <div className="space-y-2">
      <div className="animate-pulse rounded bg-muted h-4 w-24" />
      <div className="animate-pulse rounded bg-muted h-8 w-full" />
    </div>
  );
}

export function PageSkeleton({ title }: { title: string }) {
  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <h1 className="text-2xl font-bold">{title}</h1>
      <ListSkeleton count={4} />
    </div>
  );
}
