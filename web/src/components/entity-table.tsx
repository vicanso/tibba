import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import request from "@/helpers/request";
import { ChevronDown } from "lucide-react";

import { INNER_ENTITY_DESCRIPTIONS, INNER_ENTITIES } from "@/url";
import {
  ColumnDef,
  VisibilityState,
  flexRender,
  getCoreRowModel,
  useReactTable,
  PaginationState,
} from "@tanstack/react-table";
import { useEffect, useMemo, useState } from "react";
import { useAsync } from "react-async-hook";
import { useToast } from "@/components/ui/use-toast";
import { formatDate, formatError } from "@/helpers/util";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

interface EntityItem {
  name: string;
  label: string;
  category: string;
  readonly: boolean;
  width: number;
}

function convertDescriptionToColumnDef(
  items: EntityItem[],
): ColumnDef<Map<string, unknown>>[] {
  return items.map((item) => {
    const style = {
      minWidth: "",
    };
    if (item.width) {
      style.minWidth = `${item.width}px`;
    }
    return {
      accessorKey: item.name,
      header: () => {
        const name = item.label || item.name;
        if (item.category == opName) {
          return <div className="text-center sticky right-0">{name}</div>;
        }
        return name;
      },
      cell: ({ row }) => {
        if (item.category == opName) {
          return (
            <div className="grid items-center w-full" style={style}>
              <Button variant="link">编辑</Button>
            </div>
          );
        }
        let value: string = row.getValue(item.name);
        switch (item.category) {
          case "datetime": {
            value = formatDate(value);
            break;
          }
          case "status": {
            if (String(value) === "1") {
              value = "启用";
            } else {
              value = "禁用";
            }
            break;
          }
          default:
            break;
        }
        return <div style={style}>{value}</div>;
      },
    };
  });
}

const opName = "op";

async function getEntityDescriptions(entity: string): Promise<EntityItem[]> {
  const { data } = await request.get<{
    items: EntityItem[];
  }>(INNER_ENTITY_DESCRIPTIONS, {
    params: {
      table: entity,
    },
  });
  data.items.push({
    name: opName,
    label: "操作",
    category: opName,
    readonly: true,
    width: 60,
  });
  return data.items;
}

async function getEntities({
  entity,
  page,
  page_size,
  keyword,
}: {
  entity: string;
  page: number;
  page_size: number;
  keyword: string;
}) {
  const { data } = await request.get<{
    page_count: number;
    items: Map<string, unknown>[];
  }>(INNER_ENTITIES, {
    params: {
      table: entity,
      page_size,
      page,
      keyword,
    },
  });
  return data;
}

function getColumnVisibility(entity: string) {
  const value = window.localStorage.getItem(`columnVisibility:${entity}`);
  if (value) {
    try {
      return JSON.parse(value);
    } catch (err) {
      console.error(err);
    }
  }
  return {};
}

function saveColumnVisibility(entity: string, data: Record<string, boolean>) {
  window.localStorage.setItem(
    `columnVisibility:${entity}`,
    JSON.stringify(data),
  );
}

export default function DataTable({ entity }: { entity: string }) {
  const { toast } = useToast();
  const [initialized, setInitialized] = useState(false);
  const [entityItems, setEntityItems] = useState<
    ColumnDef<Map<string, unknown>>[]
  >([]);
  const [labels, setLabels] = useState(new Map<string, string>());
  const [pageCount, setPageCount] = useState(0);
  const [entities, setEntities] = useState<Map<string, unknown>[]>([]);
  const [{ pageIndex, pageSize }, setPagination] = useState<PaginationState>({
    pageIndex: -1,
    pageSize: 10,
  });
  const [columnVisibility, setColumnVisibility] = useState<VisibilityState>(
    getColumnVisibility(entity),
  );
  const [loading, setLoading] = useState(false);
  const [keyword, setKeyword] = useState("");
  let inputKeyword = "";

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
      columnVisibility,
    },
    pageCount,
    columns: entityItems,
    autoResetPageIndex: false,
    onColumnVisibilityChange: setColumnVisibility,
    getCoreRowModel: getCoreRowModel(),
    onPaginationChange: setPagination,
  });

  function reset() {
    setKeyword("");
    setInitialized(false);
    setLabels(new Map<string, string>());
    setEntityItems([]);
    setPageCount(0);
    setEntities([]);
    setPagination({
      pageSize,
      pageIndex: -1,
    });
    setColumnVisibility(getColumnVisibility(entity));
  }

  useAsync(async () => {
    reset();
    try {
      const items = await getEntityDescriptions(entity);
      const columnLabels = new Map<string, string>();
      items.forEach((item) => {
        if (item.label) {
          columnLabels.set(item.name, item.label);
        }
      });
      setLabels(columnLabels);

      setEntityItems(convertDescriptionToColumnDef(items));
      setPagination({
        pageIndex: 0,
        pageSize,
      });
    } catch (err) {
      toast({
        title: "获取实体描述信息失败",
        description: formatError(err),
      });
      console.error(err);
    } finally {
      setInitialized(true);
    }
  }, [entity]);

  useAsync(async () => {
    if (pageIndex < 0) {
      return;
    }
    if (loading) {
      return;
    }
    setLoading(true);
    try {
      const result = await getEntities({
        entity,
        page_size: pageSize,
        page: pageIndex,
        keyword,
      });
      setEntities(result.items);
      if (result.page_count >= 0) {
        setPageCount(result.page_count);
      }
    } catch (err) {
      toast({
        title: "加载数据失败",
        description: formatError(err),
      });
      console.error(err);
    } finally {
      setLoading(false);
    }
  }, [pageIndex, keyword]);

  const submitSearch = () => {
    setPagination({
      pageIndex: 0,
      pageSize,
    });
    setKeyword(inputKeyword);
  };

  useEffect(() => {
    saveColumnVisibility(entity, columnVisibility);
  }, [entity, columnVisibility]);

  if (!initialized) {
    return <div>Loading...</div>;
  }

  const tableHeader = table.getHeaderGroups().map((headerGroup) => {
    return (
      <TableRow key={headerGroup.id}>
        {headerGroup.headers.map((header) => {
          return (
            <TableHead className="p-2" key={header.id}>
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
      <div className="m-5">
        <div className="flex items-center py-4">
          <Input
            placeholder="请输入关键字"
            onChange={(event) => (inputKeyword = event.target.value)}
            onKeyDown={(event) => {
              if (event.code === "Enter") {
                submitSearch();
              }
            }}
            defaultValue={keyword}
            className="max-w-sm"
          />
          <Button
            disabled={loading}
            type="submit"
            className="ml-5 w-40"
            onClick={submitSearch}
          >
            {loading && "加载中..."}
            {!loading && "查询"}
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" className="ml-auto">
                展示的列 <ChevronDown className="ml-2 h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {table
                .getAllColumns()
                .filter((column) => column.getCanHide())
                .map((column) => {
                  return (
                    <DropdownMenuCheckboxItem
                      key={column.id}
                      checked={column.getIsVisible()}
                      onCheckedChange={(value) =>
                        column.toggleVisibility(!!value)
                      }
                    >
                      {labels.get(column.id) || column.id}
                    </DropdownMenuCheckboxItem>
                  );
                })}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        <div className="rounded-md border mb-5">
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
                      <TableCell key={cell.id} className="p-2">
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
                    无匹配数据
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </div>
        <div className="flex items-center justify-end space-x-2">
          {table.getPageCount() > 0 && (
            <div className="flex-1 text-sm text-muted-foreground">
              页数: {pageIndex + 1} / {table.getPageCount()}
            </div>
          )}
          <div className="space-x-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => table.previousPage()}
              disabled={!table.getCanPreviousPage()}
            >
              上一页
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => table.nextPage()}
              disabled={!table.getCanNextPage()}
            >
              下一页
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
