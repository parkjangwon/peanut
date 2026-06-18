"use client";

import { Pencil, Save, Trash2 } from "lucide-react";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslations } from "next-intl";
import { toast } from "sonner";

import { apiFetch, AppSummary, SdkStorageObjectSummary, StorageBucket } from "@/lib/api";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

import { DataTableView } from "../shared/data-table-view";
import { Panel, Section } from "../shared/layout-primitives";
import { formatBytes, storageMimeTypes } from "../utils/display";

export function StorageView({ app }: { app: AppSummary }) {
  const t = useTranslations("storageView");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const [selectedBucket, setSelectedBucket] = useState("");
  const buckets = useQuery({
    queryKey: ["storage", "buckets", app.id],
    queryFn: async () => (await apiFetch<{ buckets: StorageBucket[] }>(`/api/apps/${app.id}/storage/buckets`)).buckets,
  });
  const activeBucket = selectedBucket || buckets.data?.[0]?.name || "";
  const objects = useQuery({
    queryKey: ["storage", "objects", app.id, activeBucket],
    queryFn: async () =>
      (await apiFetch<{ objects: SdkStorageObjectSummary[] }>(
        `/api/apps/${app.id}/storage/buckets/${activeBucket}/objects`,
      )).objects,
    enabled: Boolean(activeBucket),
  });
  const [name, setName] = useState("assets");
  const [objectKey, setObjectKey] = useState("hello.txt");
  const [objectBody, setObjectBody] = useState("Hello Peanut");
  const [contentType, setContentType] = useState("text/plain");
  const createBucket = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${app.id}/storage/buckets`, {
        method: "POST",
        body: JSON.stringify({
          name,
          public_read: false,
          allow_client_uploads: false,
          max_object_bytes: null,
          allowed_mime_types: [],
        }),
    }),
    onSuccess: () => {
      toast.success(t("bucketCreated"));
      queryClient.invalidateQueries({ queryKey: ["storage", "buckets", app.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const uploadObject = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${app.id}/storage/buckets/${activeBucket}/objects/${objectKey}`, {
        method: "PUT",
        headers: { "Content-Type": contentType || "text/plain" },
        body: objectBody,
      }),
    onSuccess: () => {
      toast.success(t("objectUploaded"));
      queryClient.invalidateQueries({ queryKey: ["storage", "objects", app.id, activeBucket] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  return (
    <Section title={t("title")} description={t("description")}>
      <Panel title={t("createBucket")}>
        <div className="flex gap-3">
          <Input value={name} onChange={(event) => setName(event.target.value)} />
          <Button onClick={() => createBucket.mutate()} disabled={createBucket.isPending}>{common("create")}</Button>
        </div>
      </Panel>
      <Tabs defaultValue="buckets">
        <TabsList>
          <TabsTrigger value="buckets">{t("buckets")}</TabsTrigger>
          <TabsTrigger value="objects">{t("objects")}</TabsTrigger>
        </TabsList>
        <TabsContent value="buckets">
          <DataTableView
            loading={buckets.isLoading}
            columns={[t("name"), t("publicRead"), t("clientUploads"), t("maxBytes"), t("updated"), t("columnsActions")]}
            rows={(buckets.data ?? []).map((bucket) => [
              bucket.name,
              bucket.public_read ? common("yes") : common("no"),
              bucket.allow_client_uploads ? common("yes") : common("no"),
              bucket.max_object_bytes ? formatBytes(bucket.max_object_bytes) : common("no"),
              bucket.updated_at,
              <BucketActions key={bucket.name} appId={app.id} bucket={bucket} />,
            ])}
            emptyTitle={t("emptyBucketsTitle")}
            emptyDescription={t("emptyBucketsDescription")}
          />
        </TabsContent>
        <TabsContent value="objects" className="space-y-4">
          <Panel title={t("overwriteObject")}>
            <div className="grid gap-3 lg:grid-cols-[220px_220px_180px_1fr_auto]">
              <Select value={activeBucket} onValueChange={setSelectedBucket}>
                <SelectTrigger><SelectValue placeholder={t("selectBucket")} /></SelectTrigger>
                <SelectContent>
                  {(buckets.data ?? []).map((bucket) => (
                    <SelectItem key={bucket.name} value={bucket.name}>{bucket.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Input value={objectKey} onChange={(event) => setObjectKey(event.target.value)} />
              <Input value={contentType} onChange={(event) => setContentType(event.target.value)} placeholder={t("contentType")} />
              <Input value={objectBody} onChange={(event) => setObjectBody(event.target.value)} placeholder={t("objectBody")} />
              <Button onClick={() => uploadObject.mutate()} disabled={!activeBucket || uploadObject.isPending}>{common("upload")}</Button>
            </div>
          </Panel>
          <DataTableView
            loading={objects.isLoading}
            columns={[t("key"), t("size"), t("contentType"), t("updated"), t("columnsActions")]}
            rows={(objects.data ?? []).map((object) => [
              object.key,
              object.size,
              object.content_type ?? "",
              object.updated_at,
              <ObjectActions key={object.key} appId={app.id} bucket={activeBucket} objectKey={object.key} />,
            ])}
            emptyTitle={activeBucket ? t("emptyObjectsTitle") : t("emptyObjectsNoBucketTitle")}
            emptyDescription={activeBucket ? t("emptyObjectsDescription") : t("emptyObjectsNoBucketDescription")}
          />
        </TabsContent>
      </Tabs>
    </Section>
  );
}

function BucketActions({ appId, bucket }: { appId: string; bucket: StorageBucket }) {
  const t = useTranslations("storageView");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [publicRead, setPublicRead] = useState(bucket.public_read ? "true" : "false");
  const [clientUploads, setClientUploads] = useState(bucket.allow_client_uploads ? "true" : "false");
  const [maxBytes, setMaxBytes] = useState(bucket.max_object_bytes ? String(bucket.max_object_bytes) : "");
  const [mimeTypes, setMimeTypes] = useState(storageMimeTypes(bucket).join(", "));

  const updateBucket = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${appId}/storage/buckets/${bucket.name}`, {
        method: "PATCH",
        body: JSON.stringify({
          public_read: publicRead === "true",
          allow_client_uploads: clientUploads === "true",
          max_object_bytes: maxBytes.trim() ? Number(maxBytes) : null,
          allowed_mime_types: mimeTypes
            .split(",")
            .map((value) => value.trim())
            .filter(Boolean),
        }),
      }),
    onSuccess: () => {
      toast.success(t("bucketUpdated"));
      setOpen(false);
      queryClient.invalidateQueries({ queryKey: ["storage", "buckets", appId] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const deleteBucket = useMutation({
    mutationFn: () => apiFetch(`/api/apps/${appId}/storage/buckets/${bucket.name}`, { method: "DELETE" }),
    onSuccess: () => {
      toast.success(t("bucketDeleted"));
      queryClient.invalidateQueries({ queryKey: ["storage", "buckets", appId] });
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <div className="flex justify-end gap-1">
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogTrigger asChild>
          <Button variant="outline" size="sm">
            <Pencil className="h-4 w-4" /> {common("edit")}
          </Button>
        </DialogTrigger>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{t("editBucket")}</DialogTitle>
            <DialogDescription>{bucket.name}</DialogDescription>
          </DialogHeader>
          <div className="grid gap-3 sm:grid-cols-2">
            <Select value={publicRead} onValueChange={setPublicRead}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="false">{t("publicRead")}: {common("no")}</SelectItem>
                <SelectItem value="true">{t("publicRead")}: {common("yes")}</SelectItem>
              </SelectContent>
            </Select>
            <Select value={clientUploads} onValueChange={setClientUploads}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="false">{t("clientUploads")}: {common("no")}</SelectItem>
                <SelectItem value="true">{t("clientUploads")}: {common("yes")}</SelectItem>
              </SelectContent>
            </Select>
            <Input
              value={maxBytes}
              onChange={(event) => setMaxBytes(event.target.value)}
              placeholder={t("emptyNoLimit")}
            />
            <Input
              value={mimeTypes}
              onChange={(event) => setMimeTypes(event.target.value)}
              placeholder={t("mimeTypesPlaceholder")}
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setOpen(false)}>{common("cancel")}</Button>
            <Button onClick={() => updateBucket.mutate()} disabled={updateBucket.isPending}>
              <Save className="h-4 w-4" /> {common("save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Button
        variant="destructive"
        size="sm"
        disabled={deleteBucket.isPending}
        onClick={() => {
          if (window.confirm(t("confirmDeleteBucket", { name: bucket.name }))) {
            deleteBucket.mutate();
          }
        }}
      >
        <Trash2 className="h-4 w-4" /> {common("delete")}
      </Button>
    </div>
  );
}

function ObjectActions({ appId, bucket, objectKey }: { appId: string; bucket: string; objectKey: string }) {
  const t = useTranslations("storageView");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const deleteObject = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${appId}/storage/buckets/${bucket}/objects/${objectKey}`, {
        method: "DELETE",
      }),
    onSuccess: () => {
      toast.success(t("objectDeleted"));
      queryClient.invalidateQueries({ queryKey: ["storage", "objects", appId, bucket] });
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <Button
      variant="destructive"
      size="sm"
      disabled={deleteObject.isPending}
      onClick={() => {
        if (window.confirm(t("confirmDeleteObject", { key: objectKey }))) {
          deleteObject.mutate();
        }
      }}
    >
      <Trash2 className="h-4 w-4" /> {common("delete")}
    </Button>
  );
}
