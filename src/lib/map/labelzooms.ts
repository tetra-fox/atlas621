import type { LabelZooms } from "$lib/core/declutter";
import type { Names } from "$lib/core/strings";
import type { Core } from "$lib/data/dataset";

import LabelsWorker from "./labels.worker?worker";
import type { LabelsRequest, LabelsResponse } from "./labels.worker";

export const loadLabelZooms = (
  core: Core,
  names: Names,
  font: string,
  zoomMax: number,
  onSnapshot: (zooms: LabelZooms) => void
): Promise<LabelZooms> =>
  new Promise((resolve, reject) => {
    const worker = new LabelsWorker();
    const request: LabelsRequest = {
      positions: core.positions.slice(),
      postCounts: core.postCounts.slice(),
      offsets: names.offsets.slice(),
      bytes: names.bytes.slice(),
      font,
      space: core.manifest.space_size,
      zoomMax
    };
    worker.onmessage = (event: MessageEvent<LabelsResponse>) => {
      const { done, ...zooms } = event.data;
      onSnapshot(zooms);
      if (done === zooms.zoom.length) {
        worker.terminate();
        resolve(zooms);
      }
    };
    worker.onerror = (event) => {
      worker.terminate();
      reject(new Error(event.message));
    };
    worker.postMessage(request, [
      request.positions.buffer,
      request.postCounts.buffer,
      request.offsets.buffer,
      request.bytes.buffer
    ]);
  });
