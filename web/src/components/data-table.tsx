import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import request from "@/request";
import { INNER_ENTITY_DESCRIPTIONS, INNER_ENTITIES } from "@/url";

import {
  ColumnDef,
  ColumnFiltersState,
  SortingState,
  VisibilityState,
  flexRender,
  getCoreRowModel,
  getFilteredRowModel,
  getPaginationRowModel,
  getSortedRowModel,
  useReactTable,
  TableState,
  PaginationState,
} from "@tanstack/react-table";
import { useMemo, useState } from "react";
import { useAsync } from "react-async-hook";
import { useToast } from "@/components/ui/use-toast";
import { formatDate, formatError } from "@/helpers/util";
import { Button } from "@/components/ui/button";

interface EntityItem {
  name: string;
  label: string;
  category: string;
  readonly: boolean;
  width: number;
}

async function getEntityDescriptions(
  entity: string,
): Promise<ColumnDef<Map<string, unknown>>[]> {
  const { data } = await request.get<{
    items: EntityItem[];
  }>(INNER_ENTITY_DESCRIPTIONS, {
    params: {
      table: entity,
    },
  });
  return data.items.map((item) => {
    const style = {
      width: "",
    };
    if (item.width) {
      style.width = `${item.width}px`;
    }
    return {
      accessorKey: item.name,
      header: item.label || item.name,
      cell: ({ row }) => {
        let value: string = row.getValue(item.name);
        if (item.category == "datetime") {
          value = formatDate(value);
        }
        return <div style={style}>{value}</div>;
      },
    };
  });
}

async function getEntities({
  entity,
  page,
  page_size,
}: {
  entity: string;
  page: number;
  page_size: number;
}) {
  const { data } = await request.get<{
    page_count: number;
    items: Map<string, unknown>[];
  }>(INNER_ENTITIES, {
    params: {
      table: entity,
      page_size,
      page,
    },
  });
  return data;
}

export default function DataTable({ entity }: { entity: string }) {
  const { toast } = useToast();
  const [initialized, setInitialized] = useState(false);
  const [entityItems, setEntityItems] = useState<
    ColumnDef<Map<string, unknown>>[]
  >([]);
  const [pageCount, setPageCount] = useState(0);
  const [entities, setEntities] = useState<Map<string, unknown>[]>([]);
  const [{ pageIndex, pageSize }, setPagination] = useState<PaginationState>({
    pageIndex: -1,
    pageSize: 10,
  });

  const pagination = useMemo(
    () => ({
      pageIndex,
      pageSize,
    }),
    [pageIndex, pageSize],
  );
  const table = useReactTable({
    data: entities,
    state: {
      pagination,
    },
    pageCount,
    columns: entityItems,
    autoResetPageIndex: false,
    getCoreRowModel: getCoreRowModel(),
    onPaginationChange: setPagination,
  });

  useAsync(async () => {
    try {
      const items = await getEntityDescriptions(entity);
      setEntityItems(items);
      setPagination({
        pageIndex: 0,
        pageSize,
      });
    } catch (err) {
      toast({
        title: "Fetch entity description fail",
        description: formatError(err),
      });
      console.error(err);
    } finally {
      setInitialized(true);
    }
  }, []);

  useAsync(async () => {
    if (pageIndex < 0) {
      return;
    }
    const result = await getEntities({
      entity,
      page_size: pageSize,
      page: pageIndex,
    });
    setEntities(result.items);
    if (result.page_count >= 0) {
      setPageCount(result.page_count);
    }
  }, [pageIndex]);

  if (!initialized) {
    return <div>Loading...</div>;
  }

  const tableHeader = table.getHeaderGroups().map((headerGroup) => {
    return (
      <TableRow key={headerGroup.id}>
        {headerGroup.headers.map((header) => {
          return (
            <TableHead key={header.id}>
              {header.isPlaceholder
                ? null
                : flexRender(
                    header.column.columnDef.header,
                    header.getContext(),
                  )}
            </TableHead>
          );
        })}
      </TableRow>
    );
  });
  return (
    <div className="w-full">
      <div className="rounded-md border m-5">
        <Table>
          <TableHeader>{tableHeader}</TableHeader>
          <TableBody>
            {table.getRowModel().rows?.length ? (
              table.getRowModel().rows.map((row) => (
                <TableRow
                  key={row.id}
                  data-state={row.getIsSelected() && "selected"}
                >
                  {row.getVisibleCells().map((cell) => (
                    <TableCell key={cell.id}>
                      {flexRender(
                        cell.column.columnDef.cell,
                        cell.getContext(),
                      )}
                    </TableCell>
                  ))}
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell
                  colSpan={entityItems.length}
                  className="h-24 text-center"
                >
                  No results.
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
      <div className="flex items-center justify-end space-x-2 m-5">
        <div className="flex-1 text-sm text-muted-foreground">
          Pages: {pageIndex + 1} / {table.getPageCount()}
        </div>
        <div className="space-x-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => table.previousPage()}
            disabled={!table.getCanPreviousPage()}
          >
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => table.nextPage()}
            disabled={!table.getCanNextPage()}
          >
            Next
          </Button>
        </div>
      </div>
    </div>
  );
}
