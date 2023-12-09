import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ChevronDown, ArrowUpDown, ArrowUp, ArrowDown } from "lucide-react";
import {
  ColumnDef,
  SortingState,
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
import useUserStore from "@/state/user";
import useEntityStore, { EntityDescription } from "@/state/entity";
import { useLocation, useSearchParams } from "react-router-dom";
import { goToEntityForm } from "@/router";

function convertDescriptionToColumnDef(
  description: EntityDescription,
  sorting: SortingState,
  roles: string[],
  entity: string,
): ColumnDef<Record<string, unknown>>[] {
  let canModify = false;
  description.modify_roles.forEach((item) => {
    if (roles.includes(item)) {
      canModify = true;
    }
  });

  return description.items.map((item) => {
    const style = {
      minWidth: "",
    };
    if (item.width) {
      style.minWidth = `${item.width}px`;
    }
    return {
      accessorKey: item.name,
      enableSorting: true,
      header: ({ column }) => {
        const name = item.label || item.name;
        if (item.category == opName) {
          return <div className="text-center sticky right-0">{name}</div>;
        }
        if (description.support_orders?.includes(item.name)) {
          let arrowType = 0;
          sorting.forEach((sortItem) => {
            if (sortItem.id === item.name) {
              if (sortItem.desc) {
                arrowType = 1;
              } else {
                arrowType = 2;
              }
            }
          });
          return (
            <Button
              variant="ghost"
              onClick={() =>
                column.toggleSorting(column.getIsSorted() === "asc")
              }
            >
              {name}
              {arrowType === 0 && <ArrowUpDown className="ml-2 h-4 w-4" />}
              {arrowType === 1 && <ArrowDown className="ml-2 h-4 w-4" />}
              {arrowType === 2 && <ArrowUp className="ml-2 h-4 w-4" />}
            </Button>
          );
        }
        return name;
      },
      cell: ({ row }) => {
        if (item.category == opName) {
          return (
            <div className="grid items-center w-full" style={style}>
              {!canModify && <Button variant="link">查看</Button>}
              {canModify && (
                <Button
                  variant="link"
                  onClick={() => {
                    goToEntityForm(entity, row.getValue("id"));
                  }}
                >
                  编辑
                </Button>
              )}
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

  const [searchParams, setSearchParams] = useSearchParams();
  const location = useLocation();

  const [roles] = useUserStore((state) => [state.roles]);

  const [fetchDescription, listEntities] = useEntityStore((state) => [
    state.fetchDescription,
    state.listEntities,
  ]);

  const [initialized, setInitialized] = useState(false);
  const [entityItems, setEntityItems] = useState<
    ColumnDef<Record<string, unknown>>[]
  >([]);
  const [labels, setLabels] = useState(new Map<string, string>());
  const [pageCount, setPageCount] = useState(0);
  const [entities, setEntities] = useState<Record<string, unknown>[]>([]);
  const [{ pageIndex, pageSize }, setPagination] = useState<PaginationState>({
    pageIndex: -1,
    pageSize: 10,
  });
  const [columnVisibility, setColumnVisibility] = useState<VisibilityState>(
    getColumnVisibility(entity),
  );
  const [sorting, setSorting] = useState<SortingState>([]);
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
      sorting,
    },
    pageCount,
    columns: entityItems,
    autoResetPageIndex: false,
    onColumnVisibilityChange: setColumnVisibility,
    onSortingChange: setSorting,
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
      const description = await fetchDescription(entity);
      description.items.push({
        name: opName,
        label: "操作",
        category: opName,
        readonly: true,
        width: 60,
      });
      const columnLabels = new Map<string, string>();
      description.items.forEach((item) => {
        if (item.label) {
          columnLabels.set(item.name, item.label);
        }
      });
      setLabels(columnLabels);
      setEntityItems(
        convertDescriptionToColumnDef(description, sorting, roles, entity),
      );
      setPagination({
        pageIndex: Number(searchParams.get("page")) || 0,
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
  }, [entity, sorting]);

  useAsync(async () => {
    if (pageIndex < 0) {
      return;
    }
    if (loading) {
      return;
    }
    setLoading(true);
    try {
      const orders = sorting
        .map((item) => {
          if (item.desc) {
            return `-${item.id}`;
          }
          return item.id;
        })
        .join(",");
      const result = await listEntities({
        entity,
        page_size: pageSize,
        page: pageIndex,
        keyword,
        orders,
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

  // 点浏览器返回
  useEffect(() => {
    if (pageIndex < 0) {
      return;
    }
    const currentPage = Number(searchParams.get("page")) || 0;
    if (pageIndex !== currentPage) {
      setPagination({
        pageIndex: currentPage,
        pageSize,
      });
    }
  }, [location]);
  // 翻页时更新url query
  useEffect(() => {
    if (pageIndex < 0) {
      return;
    }
    if (pageIndex !== (Number(searchParams.get("page")) || 0)) {
      const params = new URLSearchParams();
      params.set("page", `${pageIndex}`);
      setSearchParams(params);
    }
  }, [pageIndex]);

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
