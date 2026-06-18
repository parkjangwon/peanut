"use client";

import type React from "react";
import { useTranslations } from "next-intl";

import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { cn } from "@/lib/utils";

export function DataTableView({
  columns,
  rows,
  loading,
  emptyTitle,
  emptyDescription,
  emptyAction,
}: {
  columns: string[];
  rows: React.ReactNode[][];
  loading?: boolean;
  emptyTitle?: string;
  emptyDescription?: string;
  emptyAction?: React.ReactNode;
}) {
  const common = useTranslations("common");
  if (loading) return <Skeleton className="h-64 w-full" />;
  return (
    <div className="overflow-hidden rounded-lg border bg-card">
      <Table>
        <TableHeader>
          <TableRow>
            {columns.map((column) => (
              <TableHead key={column}>{column}</TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.length === 0 ? (
            <TableRow>
              <TableCell colSpan={columns.length} className="h-36">
                <div className="flex flex-col items-center justify-center px-4 py-8 text-center">
                  <div className="font-medium">{emptyTitle ?? common("noRecords")}</div>
                  {emptyDescription && (
                    <div className="mt-1 max-w-md text-sm leading-5 text-muted-foreground">{emptyDescription}</div>
                  )}
                  {emptyAction && <div className="mt-4">{emptyAction}</div>}
                </div>
              </TableCell>
            </TableRow>
          ) : (
            rows.map((row, index) => (
              <TableRow key={index}>
                {row.map((cell, cellIndex) => (
                  <TableCell
                    key={cellIndex}
                    className={cn(
                      "max-w-[260px] text-xs",
                      typeof cell === "string" || typeof cell === "number"
                        ? "truncate font-mono"
                        : "font-sans",
                    )}
                  >
                    {cell}
                  </TableCell>
                ))}
              </TableRow>
            ))
          )}
        </TableBody>
      </Table>
    </div>
  );
}
