import { useEffect, useRef, useState } from 'react';
import { Crop, LoaderCircle, X, ZoomIn } from 'lucide-react';

export const MAX_AVATAR_BYTES = 2 * 1024 * 1024;

const PREVIEW_SIZE = 320;
const OUTPUT_SIZE = 512;

export function AvatarCropDialog({
  file,
  busy,
  error,
  t,
  onCancel,
  onConfirm,
}: {
  file: File;
  busy: boolean;
  error: string | null;
  t: (key: string) => string;
  onCancel: () => void;
  onConfirm: (blob: Blob) => Promise<void>;
}) {
  const imageRef = useRef<HTMLImageElement | null>(null);
  const dragStateRef = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    startOffsetX: number;
    startOffsetY: number;
  } | null>(null);
  const [sourceUrl, setSourceUrl] = useState('');
  const [naturalWidth, setNaturalWidth] = useState(0);
  const [naturalHeight, setNaturalHeight] = useState(0);
  const [zoom, setZoom] = useState(1);
  const [offsetX, setOffsetX] = useState(0);
  const [offsetY, setOffsetY] = useState(0);
  const [localError, setLocalError] = useState<string | null>(null);
  const [isDragging, setIsDragging] = useState(false);

  useEffect(() => {
    const objectUrl = URL.createObjectURL(file);
    setSourceUrl(objectUrl);
    return () => URL.revokeObjectURL(objectUrl);
  }, [file]);

  const previewMetrics = getCropMetrics(naturalWidth, naturalHeight, PREVIEW_SIZE, zoom);
  const previewLeft = (PREVIEW_SIZE - previewMetrics.renderedWidth) / 2 + previewMetrics.maxOffsetX * (offsetX / 100);
  const previewTop = (PREVIEW_SIZE - previewMetrics.renderedHeight) / 2 + previewMetrics.maxOffsetY * (offsetY / 100);

  const handleConfirm = async () => {
    if (!imageRef.current || !naturalWidth || !naturalHeight) {
      return;
    }

    try {
      setLocalError(null);
      let blob = await cropToBlob(imageRef.current, naturalWidth, naturalHeight, zoom, offsetX, offsetY, 'image/png');
      if (blob.size > MAX_AVATAR_BYTES) {
        blob = await cropToBlob(imageRef.current, naturalWidth, naturalHeight, zoom, offsetX, offsetY, 'image/jpeg', 0.92);
      }
      if (blob.size > MAX_AVATAR_BYTES) {
        throw new Error(t('settings.avatarTooLarge'));
      }
      await onConfirm(blob);
    } catch (submitError: any) {
      setLocalError(submitError?.message || t('settings.avatarUploadFailed'));
    }
  };

  const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (busy || !naturalWidth || !naturalHeight || (previewMetrics.maxOffsetX === 0 && previewMetrics.maxOffsetY === 0)) {
      return;
    }
    dragStateRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      startOffsetX: offsetX,
      startOffsetY: offsetY,
    };
    setIsDragging(true);
    setLocalError(null);
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!dragStateRef.current || dragStateRef.current.pointerId !== event.pointerId) {
      return;
    }

    const deltaX = event.clientX - dragStateRef.current.startX;
    const deltaY = event.clientY - dragStateRef.current.startY;
    setOffsetX(clampOffsetPercentage(dragStateRef.current.startOffsetX, deltaX, previewMetrics.maxOffsetX));
    setOffsetY(clampOffsetPercentage(dragStateRef.current.startOffsetY, deltaY, previewMetrics.maxOffsetY));
  };

  const handlePointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    if (dragStateRef.current?.pointerId === event.pointerId) {
      dragStateRef.current = null;
      setIsDragging(false);
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/55 p-6">
      <div className="w-full max-w-3xl rounded-[32px] border border-slate-200 bg-white shadow-2xl">
        <div className="flex items-center justify-between border-b border-slate-200 px-6 py-4">
          <div>
            <div className="text-lg font-semibold text-slate-900">{t('settings.avatarCropTitle')}</div>
            <div className="mt-1 text-sm text-slate-500">{t('settings.avatarCropDesc')}</div>
          </div>
          <button type="button" onClick={onCancel} className="rounded-full bg-slate-100 p-2 text-slate-500 hover:bg-slate-200" disabled={busy}>
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="grid gap-8 px-6 py-6 md:grid-cols-[360px_1fr]">
          <div className="flex flex-col items-center">
            <div
              className={`relative h-80 w-80 overflow-hidden rounded-[32px] border border-slate-200 bg-slate-100 shadow-inner ${isDragging ? 'cursor-grabbing' : 'cursor-grab'}`}
              onPointerDown={handlePointerDown}
              onPointerMove={handlePointerMove}
              onPointerUp={handlePointerUp}
              onPointerCancel={handlePointerUp}
            >
              {sourceUrl ? (
                <img
                  ref={imageRef}
                  src={sourceUrl}
                  alt={file.name}
                  draggable={false}
                  onLoad={(event) => {
                    setNaturalWidth(event.currentTarget.naturalWidth);
                    setNaturalHeight(event.currentTarget.naturalHeight);
                  }}
                  className="absolute max-w-none select-none"
                  style={{
                    width: `${previewMetrics.renderedWidth}px`,
                    height: `${previewMetrics.renderedHeight}px`,
                    left: `${previewLeft}px`,
                    top: `${previewTop}px`,
                    touchAction: 'none',
                  }}
                />
              ) : null}
              <div className="pointer-events-none absolute inset-0 ring-1 ring-inset ring-slate-200" />
            </div>
            <div className="mt-4 inline-flex items-center gap-2 rounded-full bg-slate-100 px-3 py-1.5 text-xs font-medium text-slate-600">
              <Crop className="h-3.5 w-3.5" />
              1:1
            </div>
          </div>

          <div className="space-y-5">
            <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3 text-sm leading-6 text-slate-600">
              {t('settings.avatarHint')}
            </div>
            <div className="rounded-2xl border border-blue-100 bg-blue-50 px-4 py-3 text-sm leading-6 text-blue-700">
              {t('settings.avatarDragHint')}
            </div>

            <SliderField
              icon={<ZoomIn className="h-4 w-4" />}
              label={t('settings.avatarZoom')}
              valueLabel={`${zoom.toFixed(2)}x`}
            >
              <input
                type="range"
                min={1}
                max={3}
                step={0.01}
                value={zoom}
                onChange={(event) => {
                  setZoom(Number(event.target.value));
                  setLocalError(null);
                }}
                className="w-full"
              />
            </SliderField>

            {localError || error ? <div className="rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-600">{localError || error}</div> : null}
          </div>
        </div>

        <div className="flex flex-wrap items-center justify-end gap-3 border-t border-slate-200 px-6 py-4">
          <button
            type="button"
            onClick={onCancel}
            disabled={busy}
            className="rounded-2xl border border-slate-300 bg-white px-4 py-2.5 text-sm font-medium text-slate-600 transition hover:bg-slate-50 disabled:opacity-50"
          >
            {t('common.cancel')}
          </button>
          <button
            type="button"
            onClick={handleConfirm}
            disabled={busy || !naturalWidth || !naturalHeight}
            className="inline-flex items-center gap-2 rounded-2xl bg-blue-600 px-4 py-2.5 text-sm font-semibold text-white shadow-sm transition hover:bg-blue-700 disabled:opacity-50"
          >
            {busy ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Crop className="h-4 w-4" />}
            {busy ? t('settings.avatarUploading') : t('settings.avatarApply')}
          </button>
        </div>
      </div>
    </div>
  );
}

function SliderField({
  icon,
  label,
  valueLabel,
  children,
}: {
  icon: React.ReactNode;
  label: string;
  valueLabel: string;
  children: React.ReactNode;
}) {
  return (
    <div className="rounded-2xl border border-slate-200 bg-white px-4 py-3">
      <div className="mb-3 flex items-center justify-between gap-3 text-sm text-slate-700">
        <div className="inline-flex items-center gap-2 font-medium">
          {icon}
          <span>{label}</span>
        </div>
        <span className="text-xs text-slate-500">{valueLabel}</span>
      </div>
      {children}
    </div>
  );
}

function getCropMetrics(width: number, height: number, squareSize: number, zoom: number) {
  if (!width || !height) {
    return { renderedWidth: squareSize, renderedHeight: squareSize, maxOffsetX: 0, maxOffsetY: 0 };
  }

  const baseScale = Math.max(squareSize / width, squareSize / height);
  const renderedWidth = width * baseScale * zoom;
  const renderedHeight = height * baseScale * zoom;

  return {
    renderedWidth,
    renderedHeight,
    maxOffsetX: Math.max(0, (renderedWidth - squareSize) / 2),
    maxOffsetY: Math.max(0, (renderedHeight - squareSize) / 2),
  };
}

function clampOffsetPercentage(baseOffset: number, deltaPixels: number, maxOffset: number) {
  if (maxOffset <= 0) {
    return 0;
  }
  return Math.max(-100, Math.min(100, baseOffset + (deltaPixels / maxOffset) * 100));
}

async function cropToBlob(
  image: HTMLImageElement,
  naturalWidth: number,
  naturalHeight: number,
  zoom: number,
  offsetX: number,
  offsetY: number,
  type: 'image/png' | 'image/jpeg',
  quality?: number,
) {
  const canvas = document.createElement('canvas');
  canvas.width = OUTPUT_SIZE;
  canvas.height = OUTPUT_SIZE;
  const context = canvas.getContext('2d');
  if (!context) {
    throw new Error('Canvas unavailable');
  }

  if (type === 'image/jpeg') {
    context.fillStyle = '#ffffff';
    context.fillRect(0, 0, OUTPUT_SIZE, OUTPUT_SIZE);
  }

  const metrics = getCropMetrics(naturalWidth, naturalHeight, OUTPUT_SIZE, zoom);
  const drawX = (OUTPUT_SIZE - metrics.renderedWidth) / 2 + metrics.maxOffsetX * (offsetX / 100);
  const drawY = (OUTPUT_SIZE - metrics.renderedHeight) / 2 + metrics.maxOffsetY * (offsetY / 100);
  context.drawImage(image, drawX, drawY, metrics.renderedWidth, metrics.renderedHeight);

  const blob = await new Promise<Blob | null>((resolve) => {
    canvas.toBlob(resolve, type, quality);
  });
  if (!blob) {
    throw new Error('Crop export failed');
  }
  return blob;
}
