import { useState } from "react";

export function PosterImage({
  src,
  fallback,
  loading,
  fetchPriority = "auto",
}: {
  src?: string;
  fallback: string;
  loading: "eager" | "lazy";
  fetchPriority?: "high" | "low" | "auto";
}) {
  const [failedSrc, setFailedSrc] = useState<string>();
  if (!src || failedSrc === src) {
    return <span className="poster-fallback" aria-hidden="true">{fallback}</span>;
  }
  return (
    <img
      src={src}
      alt=""
      width="360"
      height="540"
      loading={loading}
      fetchPriority={fetchPriority}
      decoding="async"
      onError={() => setFailedSrc(src)}
    />
  );
}
