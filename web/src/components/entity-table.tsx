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
import useEntityStore, {
  EntityDescription,
  formatEntityStatus,
} from "@/state/entity";
import { goToEntityForm } from "@/router";
import { Loading } from "@/components/loading";

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
            value = formatEntityStatus(value);
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

const entityPageSizeKey = "entityPageSize";
function getPageSize() {
  const value = window.localStorage.getItem(entityPageSizeKey);
  const pageSize = Number(value);
  if (Number.isNaN(pageSize) || pageSize <= 0) {
    return 10;
  }
  return pageSize;
}

function savePageSize(pageSize: number) {
  window.localStorage.setItem(entityPageSizeKey, `${pageSize}`);
}

export default function DataTable({ entity }: { entity: string }) {
  const { toast } = useToast();

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
    pageSize: getPageSize(),
  });
  const [columnVisibility, setColumnVisibility] = useState<VisibilityState>(
    getColumnVisibility(entity),
  );
  const [sorting, setSorting] = useState<SortingState>([]);
  const [loading, setLoading] = useState(false);
  const [keyword, setKeyword] = useState("");
  const [inputKeyword, setInputKeyword] = useState("");

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
      pageSize: getPageSize(),
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
        auto_created: false,
        options: [],
        span: 0,
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
        counted: pageCount === 0,
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
  }, [pageIndex, keyword, pageSize]);

  const submitSearch = () => {
    setPagination({
      pageIndex: 0,
      pageSize,
    });
    setKeyword(inputKeyword);
  };

  useEffect(() => {
    saveColumnVisibility(entity, columnVisibility);
    savePageSize(pageSize);
  }, [entity, columnVisibility, pageSize]);

  if (!initialized) {
    return <Loading />;
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
            onChange={(event) => {
              setInputKeyword(event.target.value);
              // inputKeyword = event.target.value;
            }}
            onKeyDown={(event) => {
              if (event.code === "Enter") {
                submitSearch();
              }
            }}
            type="search"
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
          {entity != "users" && (
            <Button
              className="ml-5 w-40"
              variant="secondary"
              onClick={() => {
                goToEntityForm(entity, "0");
              }}
            >
              新增
            </Button>
          )}
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
                    {!loading && "无匹配数据"}
                    {loading && "正在加载数据，请稍候..."}
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </div>
        <div className="flex items-center justify-end space-x-2">
          <div className="space-x-3">
            {table.getPageCount() > 0 && (
              <span className="flex-1 text-sm text-muted-foreground">
                页数: {pageIndex + 1} / {table.getPageCount()}
              </span>
            )}
            <span>
              每页:{" "}
              <Input
                className="inline-flex w-[64px] h-9 ml-2"
                type="number"
                defaultValue={pageSize}
                onChange={(e) => {
                  const pageSize = Number(e.target.value);
                  if (pageSize <= 0) {
                    return;
                  }
                  setPagination({
                    pageIndex,
                    pageSize,
                  });
                  savePageSize(pageSize);
                }}
              />
            </span>
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
